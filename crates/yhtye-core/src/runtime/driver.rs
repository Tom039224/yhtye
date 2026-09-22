//! The orchestration loop: owns every agent session, executes board effects,
//! forwards agent events and wakes the orchestrator from its inbox.
//!
//! Everything runs on one task so session state is never touched concurrently;
//! slow work (starting and stopping agents) runs in `JoinSet`s and comes back
//! as loop events.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;

use super::board::{Effect, StepStart};
use super::inbox::{InboxItem, render_batch};
use super::memory_port::MemoryToolPort;
use super::orchestration::{ORCHESTRATOR_SESSION, OrchEvent, OrchestrationConfig};
use crate::acp::{AgentError, AgentEvent, AgentHandle, SpawnOptions, spawn_agent};
use crate::domain::Role;
use crate::mcp::{McpHost, McpToken, SessionBinding, ToolCallRecord};
use crate::prompts::{StepPrompt, step_prompt, system_prompt};

pub(super) enum Cmd {
    UserMessage(String),
    CancelOrchestrator,
    Shutdown(oneshot::Sender<()>),
}

/// A running agent session.
struct Live {
    handle: AgentHandle,
    token: McpToken,
    binding: SessionBinding,
    /// Prompts waiting for the current turn to end.
    queued: VecDeque<String>,
}

/// Result of a background `spawn_agent`.
struct Spawned {
    token: McpToken,
    binding: SessionBinding,
    result: Result<AgentHandle, AgentError>,
}

type AgentRx = mpsc::UnboundedReceiver<(String, AgentEvent)>;

pub(super) struct Driver {
    cfg: Arc<OrchestrationConfig>,
    host: Option<McpHost>,
    port: MemoryToolPort,
    out: mpsc::UnboundedSender<OrchEvent>,
    agent_tx: mpsc::UnboundedSender<(String, AgentEvent)>,
    agent_rx: Option<AgentRx>,
    live: HashMap<String, Live>,
    /// Prompts for sessions that are still starting.
    starting: HashMap<String, VecDeque<String>>,
    spawning: JoinSet<Spawned>,
    stopping: JoinSet<String>,
    inbox: Vec<InboxItem>,
}

/// Session key of the agent running `role` for step `step` of `task`. Implementers
/// keep one session per task; every review gets a fresh session.
fn session_key(task: &str, role: Role, step: usize) -> String {
    match role {
        Role::Reviewer => format!("{task}/review-{step}"),
        Role::Implementer | Role::Orchestrator => format!("{task}/implementer"),
    }
}

impl Driver {
    pub(super) fn new(
        cfg: Arc<OrchestrationConfig>,
        host: McpHost,
        port: MemoryToolPort,
        out: mpsc::UnboundedSender<OrchEvent>,
    ) -> Self {
        let (agent_tx, agent_rx) = mpsc::unbounded_channel();
        Self {
            cfg,
            host: Some(host),
            port,
            out,
            agent_tx,
            agent_rx: Some(agent_rx),
            live: HashMap::new(),
            starting: HashMap::new(),
            spawning: JoinSet::new(),
            stopping: JoinSet::new(),
            inbox: Vec::new(),
        }
    }

    fn emit(&self, event: OrchEvent) {
        let _ = self.out.send(event);
    }

    fn host(&self) -> Result<&McpHost, AgentError> {
        self.host.as_ref().ok_or(AgentError::Closed)
    }

    /// Starts the orchestrator session and waits until it is ready.
    pub(super) async fn start_orchestrator(&mut self) -> Result<(), AgentError> {
        let binding = SessionBinding::orchestrator(ORCHESTRATOR_SESSION, self.cfg.project.clone());
        let token = self.host()?.registry().issue(binding.clone());
        let (harness, options, events) = self.launch_parts(&binding, &token)?;
        match spawn_agent(&harness, &self.cfg.project_dir, options, events).await {
            Ok(handle) => {
                self.on_spawned(Spawned {
                    token,
                    binding,
                    result: Ok(handle),
                });
                Ok(())
            }
            Err(e) => {
                self.revoke(&token);
                Err(e)
            }
        }
    }

    /// Harness, spawn options (MCP server + role prompt) and a tagged event sender.
    fn launch_parts(
        &self,
        binding: &SessionBinding,
        token: &McpToken,
    ) -> Result<
        (
            crate::acp::HarnessConfig,
            SpawnOptions,
            mpsc::UnboundedSender<AgentEvent>,
        ),
        AgentError,
    > {
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

    pub(super) async fn run(
        mut self,
        mut cmd_rx: mpsc::UnboundedReceiver<Cmd>,
        mut effects_rx: mpsc::UnboundedReceiver<Effect>,
        mut tool_rx: mpsc::UnboundedReceiver<ToolCallRecord>,
    ) {
        let Some(mut agent_rx) = self.agent_rx.take() else {
            return;
        };
        loop {
            tokio::select! {
                biased;
                cmd = cmd_rx.recv() => match cmd {
                    Some(Cmd::UserMessage(text)) => self.inbox.push(InboxItem::user_message(text)),
                    Some(Cmd::CancelOrchestrator) => self.cancel_orchestrator(),
                    Some(Cmd::Shutdown(done)) => {
                        self.shutdown().await;
                        let _ = done.send(());
                        return;
                    }
                    None => {
                        self.shutdown().await;
                        return;
                    }
                },
                Some(effect) = effects_rx.recv() => self.on_effect(effect),
                Some(record) = tool_rx.recv() => self.emit(OrchEvent::ToolCalled(record)),
                Some(Ok(spawned)) = self.spawning.join_next() => self.on_spawned(spawned),
                Some(Ok(key)) = self.stopping.join_next() => self.emit(OrchEvent::SessionStopped { session: key }),
                Some((key, event)) = agent_rx.recv() => self.on_agent_event(key, event),
            }
            self.flush_inbox();
        }
    }

    fn cancel_orchestrator(&self) {
        if let Some(live) = self.live.get(ORCHESTRATOR_SESSION)
            && let Err(e) = live.handle.cancel()
        {
            tracing::warn!("cancel orchestrator turn: {e}");
        }
    }

    fn on_effect(&mut self, effect: Effect) {
        match effect {
            Effect::StartStep(step) => self.start_step(&step),
            Effect::Wake(item) => self.inbox.push(item),
            Effect::PromptTask { task, text } => {
                let current = self.port.with_board(|b| {
                    let t = b.task(&task)?;
                    Some((t.current, t.steps.get(t.current)?.kind.role()?))
                });
                match current {
                    Some((step, role)) => {
                        self.prompt_session(&session_key(&task, role, step), text)
                    }
                    None => tracing::warn!("no agent step is running for {task}; reply dropped"),
                }
            }
            Effect::TaskFinished { task } => self.stop_task_sessions(&task),
        }
    }

    fn start_step(&mut self, s: &StepStart) {
        let key = session_key(&s.task, s.role, s.step);
        let binding = SessionBinding {
            session: key.clone(),
            role: s.role,
            project: self.cfg.project.clone(),
            group: Some(s.group.clone()),
            task: Some(s.task.clone()),
            step: Some(s.step),
        };
        let text = step_prompt(&StepPrompt {
            task_id: &s.task,
            task_title: &s.title,
            task_kind: s.kind,
            step_index: s.step,
            step_count: s.step_count,
            step_kind: s.step_kind,
            instruction: &s.instruction,
        });
        if let Some(live) = self.live.get_mut(&key) {
            // Reused implementer session: move its token to the new step.
            if let Some(host) = &self.host {
                host.registry().rebind(&live.token, binding.clone());
            }
            live.binding = binding;
            self.prompt_session(&key, text);
        } else if let Some(queue) = self.starting.get_mut(&key) {
            queue.push_back(text);
        } else {
            self.start_session(binding, text);
        }
    }

    /// Starts an agent session in the background; `first_prompt` is sent once it is ready.
    fn start_session(&mut self, binding: SessionBinding, first_prompt: String) {
        let key = binding.session.clone();
        let parts = self.host().map(|h| h.registry().issue(binding.clone()));
        let token = match parts {
            Ok(t) => t,
            Err(e) => {
                return self.emit(OrchEvent::SessionFailed {
                    session: key,
                    error: e.to_string(),
                });
            }
        };
        let (harness, options, events) = match self.launch_parts(&binding, &token) {
            Ok(p) => p,
            Err(e) => {
                return self.emit(OrchEvent::SessionFailed {
                    session: key,
                    error: e.to_string(),
                });
            }
        };
        let cwd = self.cfg.project_dir.clone();
        self.starting.insert(key, VecDeque::from([first_prompt]));
        self.spawning.spawn(async move {
            let result = spawn_agent(&harness, &cwd, options, events).await;
            Spawned {
                token,
                binding,
                result,
            }
        });
    }

    fn on_spawned(&mut self, s: Spawned) {
        let key = s.binding.session.clone();
        let queued = self.starting.remove(&key).unwrap_or_default();
        let handle = match s.result {
            Ok(h) => h,
            Err(e) => {
                self.revoke(&s.token);
                return self.emit(OrchEvent::SessionFailed {
                    session: key,
                    error: e.to_string(),
                });
            }
        };
        self.emit(OrchEvent::SessionStarted {
            session: key.clone(),
            role: s.binding.role,
            task: s.binding.task.clone(),
            pid: handle.pid(),
        });
        let task = s.binding.task.clone();
        self.live.insert(
            key.clone(),
            Live {
                handle,
                token: s.token,
                binding: s.binding,
                queued,
            },
        );
        // The task may have been cancelled while its agent was starting.
        if task.is_some_and(|t| self.task_is_terminal(&t)) {
            self.stop(&key);
        } else {
            self.deliver(&key);
        }
    }

    fn task_is_terminal(&self, task: &str) -> bool {
        self.port
            .with_board(|b| b.task(task).is_some_and(|t| t.status.is_terminal()))
    }

    fn on_agent_event(&mut self, key: String, event: AgentEvent) {
        match &event {
            AgentEvent::TurnEnded(_) => self.on_turn_ended(&key),
            AgentEvent::Exited { code, signal } => {
                if let Some(live) = self.live.remove(&key) {
                    tracing::warn!("agent {key} exited (code {code:?}, signal {signal:?})");
                    self.revoke(&live.token);
                    self.emit(OrchEvent::SessionStopped {
                        session: key.clone(),
                    });
                }
            }
            _ => {}
        }
        self.emit(OrchEvent::Agent {
            session: key,
            event,
        });
    }

    fn on_turn_ended(&mut self, key: &str) {
        let Some(binding) = self.live.get(key).map(|l| l.binding.clone()) else {
            return;
        };
        if let Some(task) = &binding.task {
            self.port.on_turn_ended(task);
            // A reviewer session is done once its step is no longer the current one.
            let still_current = self.port.with_board(|b| {
                b.task(task)
                    .is_some_and(|t| !t.status.is_terminal() && Some(t.current) == binding.step)
            });
            if binding.role == Role::Reviewer && !still_current {
                return self.stop(key);
            }
        }
        self.deliver(key);
    }

    /// Queues `text` for session `key` and sends it if the session is idle.
    fn prompt_session(&mut self, key: &str, text: String) {
        if let Some(queue) = self.starting.get_mut(key) {
            return queue.push_back(text);
        }
        match self.live.get_mut(key) {
            Some(live) => live.queued.push_back(text),
            None => return tracing::warn!("no session {key}; prompt dropped"),
        }
        self.deliver(key);
    }

    /// Sends the next queued prompt of `key` if no turn is running.
    fn deliver(&mut self, key: &str) {
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
            Ok(()) => self.emit(OrchEvent::Prompted {
                session: key.to_string(),
                text,
            }),
            Err(e) => {
                tracing::warn!("prompt to {key} failed: {e}");
                live.queued.push_front(text);
            }
        }
    }

    /// Wakes the orchestrator with everything in the inbox if it is idle.
    fn flush_inbox(&mut self) {
        if self.inbox.is_empty() {
            return;
        }
        let Some(live) = self.live.get_mut(ORCHESTRATOR_SESSION) else {
            return;
        };
        if live.handle.is_turn_running() || !live.queued.is_empty() {
            return;
        }
        live.queued.push_back(render_batch(&self.inbox));
        self.inbox.clear();
        self.deliver(ORCHESTRATOR_SESSION);
    }

    fn stop_task_sessions(&mut self, task: &str) {
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

    /// Revokes the session's token and shuts it down in the background.
    fn stop(&mut self, key: &str) {
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
                self.emit(OrchEvent::SessionStopped { session: key });
            }
        }
        if let Some(host) = self.host.take() {
            host.shutdown().await;
        }
    }
}
