//! The runtime loop: feeds tool calls and agent events to the state machine as
//! [`DomainCommand`]s, publishes the resulting events and carries out effects.
//!
//! Everything runs on one task, so state changes are strictly ordered. Agent
//! starts and stops run in the background; git effects are awaited inline and
//! their results fed back as commands in the same chain, so a tool call is
//! answered only after its whole chain (e.g. `finish_group` → merge) is done.

use std::collections::VecDeque;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use super::emitter::Emitter;
use super::orchestration::{ORCHESTRATOR_SESSION, OrchestrationConfig};
use super::port::ToolRequest;
use super::sessions::{Sessions, Spawned, agent_ref, session_key};
use crate::acp::schema::StopReason;
use crate::acp::{AgentError, AgentEvent};
use crate::api::{ApiEventBody, Snapshot};
use crate::domain::{DomainCommand, Effect, Machine, ToolError, TurnOutcome, render_batch};
use crate::git::GitService;
use crate::mcp::{SessionBinding, ToolCall, ToolCallRecord};

pub(super) enum Cmd {
    UserMessage(String),
    CancelOrchestrator,
    Snapshot(oneshot::Sender<Snapshot>),
    Shutdown(oneshot::Sender<()>),
}

pub(super) struct Channels {
    pub(super) cmd: mpsc::UnboundedReceiver<Cmd>,
    pub(super) tools: mpsc::UnboundedReceiver<ToolRequest>,
    pub(super) records: mpsc::UnboundedReceiver<ToolCallRecord>,
    pub(super) agents: mpsc::UnboundedReceiver<(String, AgentEvent)>,
}

pub(super) struct Driver {
    cfg: Arc<OrchestrationConfig>,
    machine: Machine,
    git: Arc<dyn GitService>,
    pub(super) sessions: Sessions,
    emit: Emitter,
}

type Reply = Result<Value, ToolError>;

impl Driver {
    pub(super) fn new(
        cfg: Arc<OrchestrationConfig>,
        machine: Machine,
        sessions: Sessions,
        emit: Emitter,
    ) -> Self {
        let git = cfg.git.clone();
        Self {
            cfg,
            machine,
            git,
            sessions,
            emit,
        }
    }

    pub(super) async fn run(mut self, mut ch: Channels) {
        loop {
            tokio::select! {
                biased;
                cmd = ch.cmd.recv() => {
                    if !self.on_cmd(cmd).await {
                        return;
                    }
                }
                Some(req) = ch.tools.recv() => self.on_tool(req).await,
                Some(record) = ch.records.recv() => {
                    self.emit.send(ApiEventBody::ToolCalled { record });
                }
                Some(Ok(spawned)) = self.sessions.spawning.join_next() => self.on_spawned(spawned).await,
                Some(Ok(key)) = self.sessions.stopping.join_next() => {
                    self.emit.send(ApiEventBody::SessionStopped { session: key });
                }
                Some((key, event)) = ch.agents.recv() => self.on_agent_event(key, event).await,
            }
            self.flush_inbox().await;
        }
    }

    /// Returns `false` when the loop must end.
    async fn on_cmd(&mut self, cmd: Option<Cmd>) -> bool {
        match cmd {
            Some(Cmd::UserMessage(text)) => {
                self.execute(DomainCommand::UserMessage { text }).await;
            }
            Some(Cmd::CancelOrchestrator) => self.sessions.cancel_turn(ORCHESTRATOR_SESSION),
            Some(Cmd::Snapshot(tx)) => {
                let _ = tx.send(Snapshot {
                    seq: self.emit.seq(),
                    state: self.machine.state().clone(),
                });
            }
            Some(Cmd::Shutdown(done)) => {
                self.sessions.shutdown().await;
                let _ = done.send(());
                return false;
            }
            None => {
                self.sessions.shutdown().await;
                return false;
            }
        }
        true
    }

    async fn on_tool(&mut self, req: ToolRequest) {
        let reply = self.tool_reply(req.binding, req.call).await;
        // The caller may have gone away (e.g. its HTTP request was dropped).
        let _ = req.reply.send(reply);
    }

    async fn tool_reply(&mut self, binding: SessionBinding, call: ToolCall) -> Reply {
        let cmd = match call {
            ToolCall::CreateGroup(args) => {
                let base_branch = self.git.current_branch().await.map_err(|e| {
                    ToolError::internal(format!("could not read the current branch: {e}"))
                })?;
                DomainCommand::CreateGroup { args, base_branch }
            }
            call => DomainCommand::Tool { binding, call },
        };
        self.execute(cmd)
            .await
            .unwrap_or_else(|| Err(ToolError::internal("the command produced no reply")))
    }

    /// Applies `cmd` and everything that follows from it (git results), and
    /// returns the last tool reply produced along the way.
    async fn execute(&mut self, cmd: DomainCommand) -> Option<Reply> {
        let mut reply = None;
        let mut queue = VecDeque::from([cmd]);
        let mut first = true;
        while let Some(cmd) = queue.pop_front() {
            match self.machine.handle(cmd) {
                Ok(t) => {
                    for event in t.events {
                        self.emit.send(ApiEventBody::Domain { event });
                    }
                    if t.reply.is_some() {
                        reply = t.reply;
                    }
                    for effect in t.effects {
                        if let Some(next) = self.carry_out(effect).await {
                            queue.push_back(next);
                        }
                    }
                }
                Err(e) if first => reply = Some(Err(e)),
                Err(e) => tracing::error!("follow-up command failed: {e}"),
            }
            first = false;
        }
        reply
    }

    /// Carries out one effect; git results come back as the next command.
    async fn carry_out(&mut self, effect: Effect) -> Option<DomainCommand> {
        match effect {
            Effect::RunStep {
                agent,
                group,
                prompt,
                workdir,
            } => {
                let cwd = workdir.unwrap_or_else(|| self.cfg.project_dir.clone());
                let binding = self.binding(&agent, Some(group));
                self.sessions.prompt(binding, cwd, prompt);
            }
            Effect::PromptAgent { agent, text } => {
                let t = self.machine.state().task(&agent.task);
                let group = t.map(|t| t.group.clone());
                let cwd = t
                    .and_then(|t| t.workdir.clone())
                    .unwrap_or_else(|| self.cfg.project_dir.clone());
                let binding = self.binding(&agent, group);
                self.sessions.prompt(binding, cwd, text);
            }
            Effect::StopAgent { agent } => self.sessions.stop(&session_key(&agent)),
            Effect::StopTaskAgents { task } => self.sessions.stop_task(&task),
            Effect::Git(op) => {
                let result = self.git.run(&op).await;
                return Some(DomainCommand::GitDone { op, result });
            }
            Effect::WakeOrchestrator => {} // the inbox is flushed after every loop turn
        }
        None
    }

    fn binding(&self, agent: &crate::domain::AgentRef, group: Option<String>) -> SessionBinding {
        SessionBinding {
            session: session_key(agent),
            role: agent.role,
            project: self.cfg.project.clone(),
            group,
            task: Some(agent.task.clone()),
            step: Some(agent.step),
        }
    }

    async fn on_spawned(&mut self, spawned: Spawned) {
        let Some((binding, error)) = self.sessions.on_spawned(spawned) else {
            return;
        };
        if let Some(agent) = agent_ref(&binding) {
            self.execute(DomainCommand::AgentStartFailed { agent, error })
                .await;
        } else {
            tracing::error!("the {} session failed to start: {error}", binding.session);
        }
    }

    async fn on_agent_event(&mut self, key: String, event: AgentEvent) {
        if let AgentEvent::Stderr(line) = &event {
            tracing::debug!(session = %key, "{line}");
            return;
        }
        self.emit.send(ApiEventBody::Agent {
            session: key.clone(),
            event: event.clone(),
        });
        match event {
            AgentEvent::TurnEnded(result) => {
                let agent = self.sessions.binding(&key).and_then(agent_ref);
                if let Some(agent) = agent {
                    let outcome = turn_outcome(result);
                    let prompt_queued = self.sessions.has_queued(&key);
                    self.execute(DomainCommand::TurnEnded {
                        agent,
                        outcome,
                        prompt_queued,
                    })
                    .await;
                }
                self.sessions.deliver(&key);
            }
            AgentEvent::Exited { code, signal } => self.on_exited(&key, code, signal).await,
            _ => {}
        }
    }

    async fn on_exited(&mut self, key: &str, code: Option<i32>, signal: Option<i32>) {
        let Some(binding) = self.sessions.on_exit(key) else {
            return; // stopped by Yhtye
        };
        let detail = format!("exit code {code:?}, signal {signal:?}");
        tracing::warn!("agent {key} exited unexpectedly ({detail})");
        if let Some(agent) = agent_ref(&binding) {
            self.execute(DomainCommand::AgentExited { agent, detail })
                .await;
        }
    }

    /// Wakes the orchestrator with every undelivered inbox entry if it is idle.
    async fn flush_inbox(&mut self) {
        let inbox = &self.machine.state().inbox;
        let Some(up_to) = inbox.last().map(|e| e.id) else {
            return;
        };
        if !self.sessions.is_idle(ORCHESTRATOR_SESSION) {
            return;
        }
        let items: Vec<_> = inbox.iter().map(|e| e.item.clone()).collect();
        self.sessions
            .queue_and_deliver(ORCHESTRATOR_SESSION, render_batch(&items));
        self.execute(DomainCommand::InboxDelivered { up_to }).await;
    }
}

fn turn_outcome(result: Result<StopReason, AgentError>) -> TurnOutcome {
    match result {
        Ok(StopReason::EndTurn) => TurnOutcome::EndTurn,
        Ok(StopReason::Cancelled) => TurnOutcome::Cancelled,
        Ok(other) => TurnOutcome::Stopped(stop_reason_name(&other)),
        Err(AgentError::Closed) => TurnOutcome::Closed,
        Err(e) => TurnOutcome::Error(e.to_string()),
    }
}

fn stop_reason_name(reason: &StopReason) -> String {
    serde_json::to_value(reason)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{reason:?}"))
}
