//! Agent sessions owned by the runtime loop: starting them in the background,
//! queueing prompts until a session is idle, rebinding MCP tokens and stopping.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinSet;

use super::emitter::Emitter;
use super::orchestration::{ORCHESTRATOR_SESSION, OrchestrationConfig};
use crate::acp::{AgentError, AgentEvent, AgentHandle, HarnessConfig, SpawnOptions, spawn_agent};
use crate::api::ApiEventBody;
use crate::domain::{AgentRef, Role};
use crate::mcp::{McpHost, McpToken, SessionBinding};
use crate::prompts::system_prompt;

/// Session key of the agent for `agent`: one implementer session per task, a
/// fresh reviewer session per review step.
#[must_use]
pub fn session_key(agent: &AgentRef) -> String {
    match agent.role {
        Role::Reviewer => format!("{}/review-{}", agent.task, agent.step),
        Role::Implementer | Role::Orchestrator => format!("{}/implementer", agent.task),
    }
}

/// The step a sub-agent session is bound to.
#[must_use]
pub fn agent_ref(binding: &SessionBinding) -> Option<AgentRef> {
    Some(AgentRef {
        task: binding.task.clone()?,
        role: binding.role,
        step: binding.step?,
    })
}

struct Live {
    handle: AgentHandle,
    token: McpToken,
    binding: SessionBinding,
    /// Prompts waiting for the current turn to end.
    queued: VecDeque<String>,
}

struct Starting {
    token: McpToken,
    queued: VecDeque<String>,
}

/// Result of a background `spawn_agent`.
pub(super) struct Spawned {
    token: McpToken,
    binding: SessionBinding,
    result: Result<AgentHandle, AgentError>,
}

type Launch = (
    HarnessConfig,
    SpawnOptions,
    mpsc::UnboundedSender<AgentEvent>,
);

pub(super) struct Sessions {
    cfg: Arc<OrchestrationConfig>,
    host: Option<McpHost>,
    emit: Emitter,
    agent_tx: mpsc::UnboundedSender<(String, AgentEvent)>,
    live: HashMap<String, Live>,
    starting: HashMap<String, Starting>,
    pub(super) spawning: JoinSet<Spawned>,
    pub(super) stopping: JoinSet<String>,
}

impl Sessions {
    pub(super) fn new(
        cfg: Arc<OrchestrationConfig>,
        host: McpHost,
        emit: Emitter,
        agent_tx: mpsc::UnboundedSender<(String, AgentEvent)>,
    ) -> Self {
        Self {
            cfg,
            host: Some(host),
            emit,
            agent_tx,
            live: HashMap::new(),
            starting: HashMap::new(),
            spawning: JoinSet::new(),
            stopping: JoinSet::new(),
        }
    }

    fn host(&self) -> Result<&McpHost, AgentError> {
        self.host.as_ref().ok_or(AgentError::Closed)
    }

    pub(super) fn binding(&self, key: &str) -> Option<&SessionBinding> {
        self.live.get(key).map(|l| &l.binding)
    }

    /// Starts the orchestrator session and waits until it is ready.
    pub(super) async fn start_orchestrator(&mut self) -> Result<(), AgentError> {
        let binding = SessionBinding::orchestrator(ORCHESTRATOR_SESSION, self.cfg.project.clone());
        let token = self.host()?.registry().issue(binding.clone());
        let (harness, options, events) = self.launch_parts(&binding, &token)?;
        let result = spawn_agent(&harness, &self.cfg.project_dir, options, events).await;
        if let Err(e) = &result {
            self.revoke(&token);
            return Err(e.clone());
        }
        self.starting.insert(
            binding.session.clone(),
            Starting {
                token: token.clone(),
                queued: VecDeque::new(),
            },
        );
        self.on_spawned(Spawned {
            token,
            binding,
            result,
        });
        Ok(())
    }

    fn launch_parts(
        &self,
        binding: &SessionBinding,
        token: &McpToken,
    ) -> Result<Launch, AgentError> {
        let options = SpawnOptions {
            mcp_servers: vec![self.host()?.acp_server(token)],
            resume: None,
            system_prompt: Some(system_prompt(binding.role).to_string()),
        };
        let harness = self.cfg.harness(binding.role).clone();
        Ok((harness, options, self.forwarder(binding.session.clone())))
    }

    /// A per-session event sender whose events arrive tagged with `key`.
    fn forwarder(&self, key: String) -> mpsc::UnboundedSender<AgentEvent> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let agent_tx = self.agent_tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                if agent_tx.send((key.clone(), ev)).is_err() {
                    break;
                }
            }
        });
        tx
    }

    /// Sends `text` to the session of `binding` (bound to the given step),
    /// starting the session in `cwd` if there is none.
    pub(super) fn prompt(&mut self, binding: SessionBinding, cwd: PathBuf, text: String) {
        let key = binding.session.clone();
        if let Some(live) = self.live.get_mut(&key) {
            if live.binding != binding {
                if let Some(host) = &self.host {
                    host.registry().rebind(&live.token, binding.clone());
                }
                live.binding = binding;
            }
            live.queued.push_back(text);
            return self.deliver(&key);
        }
        if let Some(starting) = self.starting.get_mut(&key) {
            if let Some(host) = &self.host {
                host.registry().rebind(&starting.token, binding);
            }
            return starting.queued.push_back(text);
        }
        self.start(binding, cwd, text);
    }

    fn start(&mut self, binding: SessionBinding, cwd: PathBuf, first_prompt: String) {
        let key = binding.session.clone();
        let launch = self.host().map(|h| h.registry().issue(binding.clone()));
        let token = match launch {
            Ok(t) => t,
            Err(e) => return self.failed(&key, &e),
        };
        let (harness, options, events) = match self.launch_parts(&binding, &token) {
            Ok(p) => p,
            Err(e) => {
                self.revoke(&token);
                return self.failed(&key, &e);
            }
        };
        self.starting.insert(
            key,
            Starting {
                token: token.clone(),
                queued: VecDeque::from([first_prompt]),
            },
        );
        self.spawning.spawn(async move {
            let result = spawn_agent(&harness, &cwd, options, events).await;
            Spawned {
                token,
                binding,
                result,
            }
        });
    }

    fn failed(&self, key: &str, e: &AgentError) {
        self.emit.send(ApiEventBody::SessionFailed {
            session: key.to_string(),
            error: e.to_string(),
        });
    }

    /// A background start finished. Returns the binding and error if it failed.
    pub(super) fn on_spawned(&mut self, s: Spawned) -> Option<(SessionBinding, String)> {
        let key = s.binding.session.clone();
        let starting = self.starting.remove(&key);
        // The token may have been rebound while starting; use its current binding.
        let binding = self
            .host
            .as_ref()
            .and_then(|h| h.registry().get(s.token.as_str()))
            .unwrap_or(s.binding);
        let handle = match s.result {
            Ok(h) => h,
            Err(e) => {
                self.revoke(&s.token);
                self.failed(&key, &e);
                return starting.map(|_| (binding, e.to_string()));
            }
        };
        self.emit.send(ApiEventBody::SessionStarted {
            session: key.clone(),
            role: binding.role,
            task: binding.task.clone(),
            pid: handle.pid(),
        });
        let queued = starting
            .as_ref()
            .map(|s| s.queued.clone())
            .unwrap_or_default();
        self.live.insert(
            key.clone(),
            Live {
                handle,
                token: s.token,
                binding,
                queued,
            },
        );
        if starting.is_none() {
            // Stopped while it was starting.
            self.stop(&key);
        } else {
            self.deliver(&key);
        }
        None
    }

    /// Sends the next queued prompt of `key` if no turn is running.
    pub(super) fn deliver(&mut self, key: &str) {
        let Some(live) = self.live.get_mut(key) else {
            return;
        };
        if live.handle.is_turn_running() {
            return;
        }
        let Some(text) = live.queued.pop_front() else {
            return;
        };
        match live.handle.prompt_text(text.clone()) {
            Ok(()) => self.emit.send(ApiEventBody::Prompted {
                session: key.to_string(),
                text,
            }),
            Err(e) => {
                tracing::warn!("prompt to {key} failed: {e}");
                live.queued.push_front(text);
            }
        }
    }

    /// Queues `text` for the live session `key` and sends it if it is idle.
    pub(super) fn queue_and_deliver(&mut self, key: &str, text: String) {
        match self.live.get_mut(key) {
            Some(live) => live.queued.push_back(text),
            None => return tracing::warn!("no session {key}; prompt dropped"),
        }
        self.deliver(key);
    }

    /// Whether prompts are waiting for `key`'s current turn to end.
    pub(super) fn has_queued(&self, key: &str) -> bool {
        self.live.get(key).is_some_and(|l| !l.queued.is_empty())
    }

    /// Whether `key` is live, not in a turn and has nothing queued.
    pub(super) fn is_idle(&self, key: &str) -> bool {
        self.live
            .get(key)
            .is_some_and(|l| !l.handle.is_turn_running() && l.queued.is_empty())
    }

    /// The process of `key` exited. Returns its binding if Yhtye did not stop it.
    pub(super) fn on_exit(&mut self, key: &str) -> Option<SessionBinding> {
        let live = self.live.remove(key)?;
        self.revoke(&live.token);
        self.emit.send(ApiEventBody::SessionStopped {
            session: key.to_string(),
        });
        Some(live.binding)
    }

    pub(super) fn cancel_turn(&self, key: &str) {
        if let Some(live) = self.live.get(key)
            && let Err(e) = live.handle.cancel()
        {
            tracing::warn!("cancel turn of {key}: {e}");
        }
    }

    /// Stops every session of `task`, including ones still starting.
    pub(super) fn stop_task(&mut self, task: &str) {
        let prefix = format!("{task}/");
        self.starting.retain(|key, _| !key.starts_with(&prefix));
        let keys: Vec<String> = self
            .live
            .iter()
            .filter(|(_, l)| l.binding.task.as_deref() == Some(task))
            .map(|(k, _)| k.clone())
            .collect();
        for key in keys {
            self.stop(&key);
        }
    }

    /// Revokes the session's token and shuts it down in the background (a turn
    /// in progress is cancelled first; see `AgentHandle::shutdown`).
    pub(super) fn stop(&mut self, key: &str) {
        self.starting.remove(key);
        let Some(live) = self.live.remove(key) else {
            return;
        };
        self.revoke(&live.token);
        let key = key.to_string();
        self.stopping.spawn(async move {
            live.handle.shutdown().await;
            key
        });
    }

    fn revoke(&self, token: &McpToken) {
        if let Some(host) = &self.host {
            host.registry().revoke(token);
        }
    }

    /// Stops every session (including ones still starting) and the MCP server.
    pub(super) async fn shutdown(&mut self) {
        self.starting.clear();
        while let Some(res) = self.spawning.join_next().await {
            if let Ok(spawned) = res {
                self.on_spawned(spawned);
            }
        }
        let keys: Vec<String> = self.live.keys().cloned().collect();
        for key in keys {
            self.stop(&key);
        }
        while let Some(res) = self.stopping.join_next().await {
            if let Ok(key) = res {
                self.emit
                    .send(ApiEventBody::SessionStopped { session: key });
            }
        }
        if let Some(host) = self.host.take() {
            host.shutdown().await;
        }
    }
}
