//! Agent sessions owned by the runtime loop: starting them in the background,
//! queueing prompts until a session is idle, rebinding MCP tokens and stopping.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinSet;

use super::emitter::Emitter;
pub(crate) use super::launch::{AgentPick, StoredSession};
use super::launch::{FirstPrompts, Picked, replace_first};
use super::orchestration::OrchestrationConfig;
use crate::acp::{AgentError, AgentEvent, AgentHandle};
use crate::agents::StartScope;
use crate::api::ApiEventBody;
use crate::domain::{AgentRef, Role};
use crate::mcp::{McpHost, McpToken, SessionBinding};

/// The prompts to send first: the queued ones, with the fallback in front if
/// the stored session was to be restored but was not (`session/load` unsupported).
fn first_queue(starting: Starting, resumed: bool) -> VecDeque<String> {
    let mut queued = starting.queued;
    if !resumed && let Some(fallback) = starting.fallback {
        replace_first(&mut queued, fallback);
    }
    queued
}

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

/// An agent event tagged with its session key and launch number.
pub(super) type AgentMsg = (String, u64, AgentEvent);

struct Live {
    /// Launch number: tells events of this process from a replaced one's.
    launch: u64,
    handle: AgentHandle,
    token: McpToken,
    binding: SessionBinding,
    /// Prompts waiting for the current turn to end.
    queued: VecDeque<String>,
}

/// A session being started in the background.
pub(super) struct Starting {
    pub(super) launch: u64,
    pub(super) token: McpToken,
    pub(super) queued: VecDeque<String>,
    /// Replaces the first queued prompt if the stored session is not restored.
    pub(super) fallback: Option<String>,
    /// The stored ACP session this start restores with `session/load`.
    pub(super) resume: Option<StoredSession>,
    /// Restores of `resume` that failed before this one (tried with a new process).
    pub(super) failed_restores: u32,
    /// The stored ACP session that could not be restored and that this start
    /// replaces with a new session.
    pub(super) lost: Option<String>,
    pub(super) cwd: PathBuf,
    /// What the session was started with (a failed restore starts it again).
    pub(super) pick: AgentPick,
    /// The harness × model it runs.
    pub(super) agent: Picked,
}

impl Starting {
    /// The stored ACP session this start gave up on: the one that failed to be
    /// restored, or the one the agent could not load at all (`restored` false
    /// although a session was to be restored).
    pub(super) fn lost_session(&self, restored: bool) -> Option<String> {
        self.lost.clone().or_else(|| {
            self.resume
                .as_ref()
                .filter(|_| !restored)
                .map(|r| r.acp_session_id.clone())
        })
    }
}

/// Result of a background `spawn_agent`.
pub(super) struct Spawned {
    pub(super) launch: u64,
    pub(super) token: McpToken,
    pub(super) binding: SessionBinding,
    pub(super) result: Result<AgentHandle, AgentError>,
}

pub(super) struct Sessions {
    pub(super) cfg: Arc<OrchestrationConfig>,
    host: Option<McpHost>,
    emit: Emitter,
    pub(super) agent_tx: mpsc::UnboundedSender<AgentMsg>,
    /// Number of the last launch (see [`AgentMsg`]).
    pub(super) launches: u64,
    live: HashMap<String, Live>,
    pub(super) starting: HashMap<String, Starting>,
    /// Stored sessions to restore (`session/load`), by session key.
    pub(super) resume: HashMap<String, StoredSession>,
    pub(super) spawning: JoinSet<Spawned>,
    /// This project's access to the start permits, which the catalog shares by
    /// every project ([`crate::agents::MAX_CONCURRENT_STARTS`] at a time); closed
    /// on shutdown, which fails the starts of this project still waiting.
    pub(super) start_slots: StartScope,
    /// Shutdowns in progress; each yields its session key and launch number.
    pub(super) stopping: JoinSet<(String, u64)>,
}

impl Sessions {
    pub(super) fn new(
        cfg: Arc<OrchestrationConfig>,
        host: McpHost,
        emit: Emitter,
        agent_tx: mpsc::UnboundedSender<AgentMsg>,
        resume: HashMap<String, StoredSession>,
    ) -> Self {
        let start_slots = cfg.agents.start_slots().scope();
        Self {
            cfg,
            host: Some(host),
            emit,
            agent_tx,
            launches: 0,
            live: HashMap::new(),
            starting: HashMap::new(),
            resume,
            spawning: JoinSet::new(),
            start_slots,
            stopping: JoinSet::new(),
        }
    }

    pub(super) fn host(&self) -> Result<&McpHost, AgentError> {
        self.host.as_ref().ok_or(AgentError::Closed)
    }

    pub(super) fn binding(&self, key: &str) -> Option<&SessionBinding> {
        self.live.get(key).map(|l| &l.binding)
    }

    /// Whether an event of launch `launch` of `key` comes from a process that was
    /// replaced (e.g. a failed start followed by a new one under the same key).
    pub(super) fn is_stale(&self, key: &str, launch: u64) -> bool {
        let current = self
            .live
            .get(key)
            .map(|l| l.launch)
            .or_else(|| self.starting.get(key).map(|s| s.launch));
        current.is_some_and(|c| c != launch)
    }

    /// Sends `text` to the session of `binding` (bound to the given step),
    /// starting the session in `cwd` if there is none.
    pub(super) fn prompt(
        &mut self,
        binding: SessionBinding,
        cwd: PathBuf,
        text: String,
        pick: AgentPick,
    ) {
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
        let first = FirstPrompts {
            queued: VecDeque::from([text]),
            fallback: None,
        };
        self.start(binding, cwd, first, pick);
    }

    /// Continues an interrupted step: restores the step's stored session and sends
    /// `prompt`, or starts a new session with `fallback` if it cannot be restored.
    pub(super) fn resume_step(
        &mut self,
        binding: SessionBinding,
        cwd: PathBuf,
        prompt: String,
        fallback: String,
        pick: AgentPick,
    ) {
        if self.live.contains_key(&binding.session) || self.starting.contains_key(&binding.session)
        {
            return self.prompt(binding, cwd, prompt, pick);
        }
        let first = FirstPrompts {
            queued: VecDeque::from([prompt]),
            fallback: Some(fallback),
        };
        self.start(binding, cwd, first, pick);
    }

    pub(super) fn failed(&self, key: &str, e: &AgentError) {
        self.failed_text(key, e.to_string());
    }

    /// Reports that the start of `key` is given up.
    pub(super) fn failed_text(&self, key: &str, error: String) {
        self.emit_failed(key, error, false);
    }

    /// Reports a failed start of `key` that another one follows (the session is
    /// still starting; see [`ApiEventBody::SessionFailed`]).
    pub(super) fn failed_retrying(&self, key: &str, error: String) {
        self.emit_failed(key, error, true);
    }

    fn emit_failed(&self, key: &str, error: String, retrying: bool) {
        self.emit.send(ApiEventBody::SessionFailed {
            session: key.to_string(),
            error,
            retrying,
        });
    }

    /// A background start finished. Returns the binding and error if it failed.
    pub(super) fn on_spawned(&mut self, s: Spawned) -> Option<(SessionBinding, String)> {
        let key = s.binding.session.clone();
        // Only this launch's entry (the key may have been started again since).
        let starting = self
            .starting
            .get(&key)
            .is_some_and(|st| st.launch == s.launch)
            .then(|| self.starting.remove(&key))
            .flatten();
        // The token may have been rebound while starting; use its current binding.
        let binding = self
            .host
            .as_ref()
            .and_then(|h| h.registry().get(s.token.as_str()))
            .unwrap_or(s.binding);
        let handle = match s.result {
            Ok(h) => h,
            Err(e) => return self.start_failed(&s.token, binding, starting, &e),
        };
        let Some(starting) = starting else {
            // Stopped (or started again) while it was starting: not announced, so a
            // newer session under the same key is not overwritten in the store.
            self.revoke(&s.token);
            self.shut_down(key, s.launch, handle);
            return None;
        };
        self.emit.send(ApiEventBody::SessionStarted {
            session: key.clone(),
            role: binding.role,
            task: binding.task.clone(),
            pid: handle.pid(),
            acp_session_id: handle.info().acp_session_id.0.to_string(),
            resumed: handle.info().resumed,
            agent: Some(starting.agent.agent.clone()),
            replaced: starting.agent.replaced.clone(),
            cwd: Some(starting.cwd.display().to_string()),
            lost_acp_session_id: starting.lost_session(handle.info().resumed),
        });
        let queued = first_queue(starting, handle.info().resumed);
        let live = Live {
            launch: s.launch,
            handle,
            token: s.token,
            binding,
            queued,
        };
        self.live.insert(key.clone(), live);
        self.deliver(&key);
        None
    }

    /// A start failed: retry a failed restore with a new session, otherwise
    /// report it (returns the binding and error unless it was stopped meanwhile).
    fn start_failed(
        &mut self,
        token: &McpToken,
        binding: SessionBinding,
        starting: Option<Starting>,
        e: &AgentError,
    ) -> Option<(SessionBinding, String)> {
        self.revoke(token);
        match starting {
            Some(st) if st.resume.is_some() => {
                self.restore_failed(binding, st, &e.to_string());
                None
            }
            other => {
                self.failed(&binding.session, e);
                other.map(|_| (binding, e.to_string()))
            }
        }
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

    /// Sends `text` to the idle session `key` right away, bypassing the queue:
    /// fails with [`AgentError::Busy`] unless it is idle (nothing is queued for
    /// later), [`AgentError::Closed`] if it is not live.
    pub(super) fn prompt_now(&mut self, key: &str, text: &str) -> Result<(), AgentError> {
        if !self.is_live(key) {
            return Err(AgentError::Closed);
        }
        if !self.is_idle(key) {
            return Err(AgentError::Busy);
        }
        let live = self.live.get(key).ok_or(AgentError::Closed)?;
        live.handle.prompt_text(text)?;
        self.emit.send(ApiEventBody::Prompted {
            session: key.to_string(),
            text: text.to_string(),
        });
        Ok(())
    }

    /// Queues `text` for the live session `key` and sends it if it is idle.
    pub(super) fn queue_and_deliver(&mut self, key: &str, text: String) {
        match self.live.get_mut(key) {
            Some(live) => live.queued.push_back(text),
            None => return tracing::warn!("no session {key}; prompt dropped"),
        }
        self.deliver(key);
    }

    /// Whether `key` is being started in the background.
    pub(super) fn is_starting(&self, key: &str) -> bool {
        self.starting.contains_key(key)
    }

    /// Whether `key` has a running process that Yhtye has not stopped.
    pub(super) fn is_live(&self, key: &str) -> bool {
        self.live.contains_key(key)
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
            suspended: false,
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
        self.shut_down(key.to_string(), live.launch, live.handle);
    }

    /// Shuts `handle` down in the background (reported as `SessionStopped`
    /// unless the key has been started again meanwhile).
    fn shut_down(&mut self, key: String, launch: u64, handle: AgentHandle) {
        self.stopping.spawn(async move {
            handle.shutdown().await;
            (key, launch)
        });
    }

    pub(super) fn revoke(&self, token: &McpToken) {
        if let Some(host) = &self.host {
            host.registry().revoke(token);
        }
    }

    /// Stops every session (including ones still starting) and the MCP server.
    /// Sessions that were live are reported as suspended: they are restored when
    /// their task continues after the next start.
    pub(super) async fn shutdown(&mut self) {
        let mut suspended: HashSet<String> = self.starting.keys().cloned().collect();
        suspended.extend(self.live.keys().cloned());
        self.starting.clear();
        self.start_slots.close();
        while let Some(res) = self.spawning.join_next().await {
            match res {
                // A start that was still waiting for a slot never began.
                Ok(Spawned {
                    token,
                    result: Err(AgentError::Closed),
                    ..
                }) => self.revoke(&token),
                Ok(spawned) => {
                    self.on_spawned(spawned);
                }
                Err(_) => {}
            }
        }
        let keys: Vec<String> = self.live.keys().cloned().collect();
        for key in keys {
            self.stop(&key);
        }
        while let Some(res) = self.stopping.join_next().await {
            if let Ok((key, _)) = res {
                let suspended = suspended.contains(&key);
                self.emit.send(ApiEventBody::SessionStopped {
                    session: key,
                    suspended,
                });
            }
        }
        if let Some(host) = self.host.take() {
            host.shutdown().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::sync::oneshot;

    use super::*;
    use crate::acp::HarnessConfig;
    use crate::agents::{AgentCatalog, MAX_CONCURRENT_STARTS};
    use crate::domain::DomainConfig;
    use crate::git::NoopGit;
    use crate::mcp::TokenRegistry;
    use crate::runtime::port::LoopPort;

    async fn sessions(agents: &Arc<AgentCatalog>, project: &str) -> Sessions {
        let cfg = Arc::new(OrchestrationConfig {
            project: project.into(),
            project_dir: PathBuf::from("/tmp"),
            agents: agents.clone(),
            mcp_bind: "127.0.0.1:0".parse().expect("addr"),
            domain: DomainConfig::default(),
            git: Arc::new(NoopGit::new("/tmp", None)),
            db_path: PathBuf::from("/tmp/unused.sqlite3"),
        });
        let (port, _tools) = LoopPort::new();
        let host = McpHost::start(
            "127.0.0.1:0".parse().expect("addr"),
            TokenRegistry::new(),
            Arc::new(port),
            None,
        )
        .await
        .expect("mcp host");
        let (agent_tx, _agents_rx) = mpsc::unbounded_channel();
        Sessions::new(cfg, host, Emitter::default(), agent_tx, HashMap::new())
    }

    fn catalog() -> Arc<AgentCatalog> {
        let h = HarnessConfig::plain("unused", vec![]);
        Arc::new(AgentCatalog::fixed(h.clone(), h.clone(), h))
    }

    #[tokio::test]
    async fn projects_of_one_catalog_share_the_limit_on_concurrent_starts() {
        let agents = catalog();
        let (a, b) = (sessions(&agents, "a").await, sessions(&agents, "b").await);

        // `a` fills every permit with starts that do not finish ...
        let (release, held) = tokio::sync::watch::channel(false);
        let mut holders = Vec::new();
        for _ in 0..MAX_CONCURRENT_STARTS {
            let (scope, mut held) = (a.start_slots.clone(), held.clone());
            holders.push(tokio::spawn(async move {
                scope
                    .run(async move { held.wait_for(|r| *r).await.is_ok() })
                    .await
            }));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;

        // ... so a start of `b` waits instead of getting a permit of its own.
        let other = {
            let scope = b.start_slots.clone();
            tokio::spawn(async move { scope.run(async { "started" }).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!other.is_finished(), "b's start waits for a's");

        release.send(true).expect("holders are waiting");
        for holder in holders {
            assert!(holder.await.expect("joined").is_some());
        }
        assert_eq!(other.await.expect("joined"), Some("started"));
    }

    #[tokio::test]
    async fn shutting_one_project_down_does_not_close_the_starts_of_another() {
        let agents = catalog();
        let (mut a, b) = (sessions(&agents, "a").await, sessions(&agents, "b").await);
        let a_scope = a.start_slots.clone();
        a.shutdown().await;

        assert_eq!(a_scope.run(async { 1 }).await, None, "a is closed");
        assert_eq!(b.start_slots.run(async { 2 }).await, Some(2), "b is not");
    }

    #[tokio::test]
    async fn catalogs_do_not_share_permits() {
        let (one, two) = (catalog(), catalog());
        let (a, b) = (sessions(&one, "a").await, sessions(&two, "b").await);
        let (release, held) = oneshot::channel::<()>();
        let holder = {
            let scope = a.start_slots.clone();
            tokio::spawn(async move { scope.run(async { held.await.ok() }).await })
        };
        // One permit of `a`'s catalog is taken; `b` has all of its own.
        assert_eq!(b.start_slots.run(async { 3 }).await, Some(3));
        release.send(()).expect("holder is waiting");
        assert!(holder.await.expect("joined").is_some());
    }
}
