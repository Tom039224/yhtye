//! Starting agent sessions: MCP token, harness, system prompt, `session/load`
//! of a stored ACP session, and the per-session event forwarder.

use std::collections::VecDeque;
use std::path::PathBuf;

use tokio::sync::mpsc;

use super::sessions::{AgentMsg, Sessions, Spawned, Starting};
use crate::acp::schema::SessionId;
use crate::acp::{AgentError, AgentEvent, HarnessConfig, SpawnOptions};
use crate::agents::{AgentChoice, AgentRole};
use crate::mcp::{McpToken, SessionBinding};
use crate::prompts::{agent_choices_prompt, system_prompt};
use crate::secrets::spawn_agent_with_secrets;

pub(super) type Launch = (
    HarnessConfig,
    SpawnOptions,
    mpsc::UnboundedSender<AgentEvent>,
    Picked,
);

/// What a session runs, and what it was meant to run if that harness is not
/// registered (any more).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Picked {
    pub(crate) agent: AgentChoice,
    pub(crate) replaced: Option<AgentChoice>,
}

/// Which settings a session is started with: its selection role and the
/// task's allowed override (`core-design.md` §15.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentPick {
    pub(crate) role: AgentRole,
    pub(crate) over: Option<AgentChoice>,
}

impl AgentPick {
    pub(crate) fn orchestrator() -> Self {
        Self {
            role: AgentRole::Orchestrator,
            over: None,
        }
    }
}

/// A stored session to restore: its ACP session id and what it ran.
#[derive(Debug, Clone)]
pub(crate) struct StoredSession {
    pub(crate) acp_session_id: String,
    pub(crate) agent: Option<AgentChoice>,
}

/// What to send once a session started in the background is up.
pub(super) struct FirstPrompts {
    pub(super) queued: VecDeque<String>,
    /// Replaces the first queued prompt if the stored session was not restored.
    pub(super) fallback: Option<String>,
}

impl Sessions {
    /// Everything to start the session of `binding`: a restored session keeps
    /// the harness × model it ran (if still registered); otherwise `pick` is
    /// resolved against the settings in effect now.
    pub(super) fn launch_parts(
        &self,
        binding: &SessionBinding,
        token: &McpToken,
        resume: Option<StoredSession>,
        pick: &AgentPick,
        launch: u64,
    ) -> Result<Launch, AgentError> {
        let agents = &self.cfg.agents;
        let recorded = resume.as_ref().and_then(|r| r.agent.as_ref());
        let kept =
            recorded.and_then(|a| agents.launch_config(pick.role, a).map(|h| (a.clone(), h)));
        let (harness, picked) = match kept {
            Some((agent, harness)) => (
                harness,
                Picked {
                    agent,
                    replaced: None,
                },
            ),
            None => {
                let r = agents.resolve(&self.cfg.project, pick.role, pick.over.as_ref())?;
                // A restored session whose recorded harness is gone runs the
                // current choice; say so rather than only logging it.
                let replaced = r.replaced.or_else(|| recorded.cloned());
                (
                    r.harness,
                    Picked {
                        agent: r.choice,
                        replaced,
                    },
                )
            }
        };
        let replaying = resume.is_some();
        let mut prompt = system_prompt(binding.role).to_string();
        if pick.role == AgentRole::Orchestrator {
            prompt.push_str(&agent_choices_prompt(
                &agents.effective(Some(&self.cfg.project)),
            ));
        }
        let options = SpawnOptions {
            mcp_servers: vec![self.host()?.acp_server(token)],
            resume: resume.map(|r| SessionId::new(r.acp_session_id)),
            system_prompt: Some(prompt),
            secret_env: Default::default(),
        };
        let events = self.forwarder(binding.session.clone(), launch, replaying);
        Ok((harness, options, events, picked))
    }

    /// A per-session event sender whose events arrive tagged with `key`.
    ///
    /// When the session is restored with `session/load`, the agent replays its
    /// history as output before `Ready`; that is history already in the event
    /// log, so it is dropped here instead of being published as new output.
    fn forwarder(
        &self,
        key: String,
        launch: u64,
        mut replaying: bool,
    ) -> mpsc::UnboundedSender<AgentEvent> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let agent_tx: mpsc::UnboundedSender<AgentMsg> = self.agent_tx.clone();
        tokio::spawn(async move {
            let mut replayed = 0usize;
            while let Some(ev) = rx.recv().await {
                if replaying {
                    match ev {
                        AgentEvent::Output(_) => {
                            replayed += 1;
                            continue;
                        }
                        AgentEvent::Ready(_) => {
                            replaying = false;
                            tracing::debug!(session = %key, "dropped {replayed} replayed history updates");
                        }
                        _ => {}
                    }
                }
                if agent_tx.send((key.clone(), launch, ev)).is_err() {
                    break;
                }
            }
        });
        tx
    }

    pub(super) fn next_launch(&mut self) -> u64 {
        self.launches += 1;
        self.launches
    }

    /// The stored session to restore for `key`, if any (used once).
    pub(super) fn take_resume(&mut self, key: &str) -> Option<StoredSession> {
        self.resume.remove(key)
    }

    /// Starts the session of `binding` in the background (restoring its stored
    /// ACP session if there is one); `first` is sent once it is up.
    pub(super) fn start(
        &mut self,
        binding: SessionBinding,
        cwd: PathBuf,
        first: FirstPrompts,
        pick: AgentPick,
    ) {
        let resume = self.take_resume(&binding.session);
        self.launch(binding, cwd, first, resume, pick);
    }

    pub(super) fn launch(
        &mut self,
        binding: SessionBinding,
        cwd: PathBuf,
        mut first: FirstPrompts,
        resume: Option<StoredSession>,
        pick: AgentPick,
    ) {
        let key = binding.session.clone();
        if resume.is_none()
            && let Some(fallback) = first.fallback.take()
        {
            replace_first(&mut first.queued, fallback);
        }
        let token = match self.host().map(|h| h.registry().issue(binding.clone())) {
            Ok(t) => t,
            Err(e) => return self.failed(&key, &e),
        };
        let resuming = resume.is_some();
        let launch = self.next_launch();
        let (harness, options, events, agent) =
            match self.launch_parts(&binding, &token, resume, &pick, launch) {
                Ok(p) => p,
                Err(e) => {
                    self.revoke(&token);
                    return self.failed(&key, &e);
                }
            };
        let starting = Starting {
            launch,
            token: token.clone(),
            queued: first.queued,
            fallback: first.fallback,
            resuming,
            cwd: cwd.clone(),
            pick,
            agent,
        };
        self.starting.insert(key, starting);
        let secrets = self.cfg.agents.secrets();
        self.spawning.spawn(async move {
            let result = spawn_agent_with_secrets(&secrets, &harness, &cwd, options, events).await;
            Spawned {
                launch,
                token,
                binding,
                result,
            }
        });
    }

    /// A restore (`session/load`) failed: start a new session for the same step.
    pub(super) fn retry_fresh(&mut self, binding: SessionBinding, starting: Starting, error: &str) {
        tracing::warn!(
            "restoring session {} failed ({error}); starting a new session",
            binding.session
        );
        self.failed_text(
            &binding.session,
            format!("could not restore the session ({error}); starting a new session"),
        );
        let first = FirstPrompts {
            queued: starting.queued,
            fallback: starting.fallback,
        };
        self.launch(binding, starting.cwd, first, None, starting.pick);
    }
}

pub(super) fn replace_first(queued: &mut VecDeque<String>, text: String) {
    match queued.front_mut() {
        Some(first) => *first = text,
        None => queued.push_back(text),
    }
}
