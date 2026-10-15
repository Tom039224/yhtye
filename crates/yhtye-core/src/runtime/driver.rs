//! The runtime loop: feeds tool calls and agent events to the state machine as
//! [`DomainCommand`]s, publishes the resulting events and carries out effects.
//!
//! Everything runs on one task, so state changes are strictly ordered. Agent
//! starts and stops run in the background; git effects are awaited inline and
//! their results fed back as commands in the same chain, so a tool call is
//! answered only after its whole chain (e.g. `finish_group` → merge) is done.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use super::agent_args::{agents_status, choose_agents};
use super::chats::{ChatTarget, Orchestrators};
use super::emitter::{Emitter, Publisher};
use super::messages::Deliveries;
use super::orchestration::OrchestrationConfig;
use super::port::ToolRequest;
use super::sessions::{AgentMsg, AgentPick, Sessions, Spawned, agent_ref, session_key};
use super::transcript::Transcript;
use crate::acp::schema::StopReason;
use crate::acp::{AgentError, AgentEvent};
use crate::agents::AgentRole;
use crate::api::{ApiEventBody, Snapshot};
use crate::domain::{
    AgentRef, Chat, DomainCommand, Effect, Role, State, TaskStatus, ToolError, TurnOutcome,
    chat_of_session, decide, orchestrator_session,
};
use crate::git::GitService;
use crate::mcp::{SessionBinding, ToolCall, ToolCallRecord};
use crate::prompts::workspace_changes_note;
use crate::store::{SessionStatus, StoreError};

/// Session key of tool calls made by the user through the API (not an agent).
pub const USER_SESSION: &str = "user";

#[allow(clippy::large_enum_variant)] // `UserTool` carries a `create_task`-sized call
pub(super) enum Cmd {
    /// A message from the user to the orchestrator of a chat.
    UserMessage(String, String, oneshot::Sender<Result<(), ToolError>>),
    /// Cancels the running turn of a chat's orchestrator (`not_found`: no such chat).
    CancelOrchestrator(String, oneshot::Sender<Result<(), ToolError>>),
    /// The user opened a chat: its orchestrator is started (restored) if it has
    /// anything to resume. Answers whether it is starting (`not_found`: no such chat).
    OpenChat(String, oneshot::Sender<Result<bool, ToolError>>),
    /// Sends `/compact` to a chat's idle orchestrator.
    CompactChat(String, oneshot::Sender<Result<(), ToolError>>),
    /// Stops a chat's orchestrator session (`not_found`: no such chat;
    /// `invalid_state`: it is starting or in a turn).
    SleepChat(String, oneshot::Sender<Result<(), ToolError>>),
    /// Stops every chat's orchestrator session (`invalid_state`, with nothing
    /// stopped, if one is starting or in a turn).
    SleepProject(oneshot::Sender<Result<(), ToolError>>),
    /// A new chat in a worktree (given, or the branch's).
    CreateChat(ChatTarget, oneshot::Sender<Result<Chat, ToolError>>),
    /// The user renames a chat (chat, title).
    RenameChat(String, String, oneshot::Sender<Result<(), ToolError>>),
    /// The user deletes a chat.
    DeleteChat(String, oneshot::Sender<Result<(), ToolError>>),
    /// A new branch (name, start point) and its first chat.
    CreateBranch(
        String,
        Option<String>,
        oneshot::Sender<Result<Chat, ToolError>>,
    ),
    /// An orchestrator tool (`cancel_task` / `cancel_group`) invoked by the user.
    UserTool(ToolCall, oneshot::Sender<Reply>),
    /// The user retries the base merge of a `merge_blocked` group.
    RetryGroupMerge(String, oneshot::Sender<Reply>),
    Snapshot(oneshot::Sender<Result<Snapshot, StoreError>>),
    /// Whether work is on in the project and whether one of its chats has an
    /// orchestrator session up (see [`Driver::activity`]).
    Activity(oneshot::Sender<Result<(bool, bool), StoreError>>),
    Shutdown(oneshot::Sender<()>),
}

pub(super) struct Channels {
    pub(super) cmd: mpsc::UnboundedReceiver<Cmd>,
    pub(super) tools: mpsc::UnboundedReceiver<ToolRequest>,
    pub(super) records: mpsc::UnboundedReceiver<ToolCallRecord>,
    pub(super) agents: mpsc::UnboundedReceiver<AgentMsg>,
}

pub(super) struct Driver {
    pub(super) cfg: Arc<OrchestrationConfig>,
    /// The current domain state; replaced only after its transition is stored.
    pub(super) state: State,
    pub(super) git: Arc<dyn GitService>,
    pub(super) sessions: Sessions,
    pub(super) publisher: Publisher,
    emit: Emitter,
    transcript: Transcript,
    /// Sessions Yhtye stopped, reported as `SessionStopped` in order (§8, Stage 6a).
    stops: StopOrder,
    /// The orchestrators being started and what they were sent (§17.3).
    pub(super) orchestrators: Orchestrators,
    /// `message_task` messages waiting to be delivered.
    pub(super) deliveries: Deliveries,
}

/// Orders `SessionStopped` of a session Yhtye stopped after its last event.
///
/// The stop task finishes once the agent has emitted `Exited`, but its events
/// reach the loop through a separate forwarder and channel, so the loop can see
/// the stop finish before the session's final `TurnEnded` / `Exited`. The stop
/// is published only once both are known: the stop task finished and the
/// session's `Exited` was processed.
#[derive(Default)]
struct StopOrder {
    /// Stop finished, `Exited` not yet processed.
    awaiting_exit: HashSet<(String, u64)>,
    /// `Exited` processed for a session that is no longer live, stop not finished.
    exited: HashSet<(String, u64)>,
}

impl StopOrder {
    /// The stop task of `(key, launch)` finished; `true` if it can be published now.
    fn stop_finished(&mut self, key: &str, launch: u64) -> bool {
        let id = (key.to_string(), launch);
        if self.exited.remove(&id) {
            return true;
        }
        self.awaiting_exit.insert(id);
        false
    }

    /// `Exited` of a stopped session was processed; `true` if its stop is due.
    fn exited(&mut self, key: &str, launch: u64) -> bool {
        let id = (key.to_string(), launch);
        if self.awaiting_exit.remove(&id) {
            return true;
        }
        self.exited.insert(id);
        false
    }
}

pub(super) type Reply = Result<Value, ToolError>;

impl Driver {
    pub(super) fn new(
        cfg: Arc<OrchestrationConfig>,
        state: State,
        sessions: Sessions,
        publisher: Publisher,
    ) -> Self {
        let git = cfg.git.clone();
        let emit = publisher.emitter();
        Self {
            cfg,
            state,
            git,
            sessions,
            publisher,
            emit,
            transcript: Transcript::default(),
            stops: StopOrder::default(),
            orchestrators: Orchestrators::default(),
            deliveries: Deliveries::default(),
        }
    }

    pub(super) async fn run(mut self, mut ch: Channels, restarted: bool) {
        self.publisher.flush().await;
        if restarted {
            self.restart().await;
            self.flush_inbox().await;
            self.publisher.flush().await;
        }
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
                Some(res) = self.sessions.spawning.join_next() => match res {
                    Ok(spawned) => self.on_spawned(spawned).await,
                    Err(e) => tracing::error!("an agent start task failed: {e}"),
                },
                Some(res) = self.sessions.stopping.join_next() => match res {
                    // A newer session under the same key must not be marked stopped.
                    Ok((key, launch)) if !self.sessions.is_stale(&key, launch) => {
                        if self.stops.stop_finished(&key, launch) {
                            self.emit.send(ApiEventBody::SessionStopped { session: key, suspended: false });
                        }
                    }
                    Ok(_) => {}
                    Err(e) => tracing::error!("an agent stop task failed: {e}"),
                },
                Some(res) = self.deliveries.wakes.join_next() => match res {
                    Ok(wake) => self.on_message_wake(wake).await,
                    Err(e) => tracing::error!("a message delivery task failed: {e}"),
                },
                Some((key, launch, event)) = ch.agents.recv() => {
                    if self.sessions.is_stale(&key, launch) {
                        tracing::debug!(session = %key, "dropping an event of a replaced process: {event:?}");
                    } else {
                        let exited = matches!(event, AgentEvent::Exited { .. });
                        let stopped = exited && !self.sessions.is_live(&key);
                        self.on_agent_event(key.clone(), event).await;
                        if stopped && self.stops.exited(&key, launch) {
                            self.emit.send(ApiEventBody::SessionStopped { session: key, suspended: false });
                        }
                    }
                }
            }
            self.settle_messages().await;
            self.flush_inbox().await;
            self.publisher.flush().await;
        }
    }

    /// Marks what was in flight as interrupted and resumes each interrupted task.
    /// The chats' orchestrators are not started here (`core-design.md` §17.3): a
    /// chat's starts when it is opened or when something is sent to it — the
    /// inbox entries left over (also the ones this restart queues) are flushed by
    /// the caller, which starts the chats that have any.
    async fn restart(&mut self) {
        self.execute(DomainCommand::Restart).await;
        let interrupted: Vec<String> = self
            .state
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Interrupted)
            .map(|t| t.id.clone())
            .collect();
        for task in interrupted {
            self.execute(DomainCommand::ResumeTask { task }).await;
        }
    }

    /// Returns `false` when the loop must end.
    async fn on_cmd(&mut self, cmd: Option<Cmd>) -> bool {
        match cmd {
            Some(Cmd::UserMessage(chat, text, tx)) => {
                let _ = tx.send(self.user_message(chat, text).await);
            }
            Some(Cmd::CancelOrchestrator(chat, tx)) => {
                let reply = match self.state.chat(&chat) {
                    Some(_) => {
                        self.sessions.cancel_turn(&orchestrator_session(&chat));
                        Ok(())
                    }
                    None => Err(ToolError::not_found(format!("no chat {chat}"))),
                };
                let _ = tx.send(reply);
            }
            Some(Cmd::OpenChat(chat, tx)) => {
                let _ = tx.send(self.open_chat(chat).await);
            }
            Some(Cmd::CompactChat(chat, tx)) => {
                let _ = tx.send(self.compact_chat(&chat));
            }
            Some(Cmd::SleepChat(chat, tx)) => {
                let _ = tx.send(self.sleep_chat(&chat));
            }
            Some(Cmd::SleepProject(tx)) => {
                let _ = tx.send(self.sleep_project());
            }
            Some(Cmd::CreateChat(target, tx)) => {
                let _ = tx.send(self.create_chat(target).await);
            }
            Some(Cmd::RenameChat(chat, title, tx)) => {
                let _ = tx.send(self.rename_chat(chat, title).await);
            }
            Some(Cmd::DeleteChat(chat, tx)) => {
                let _ = tx.send(self.delete_chat(chat).await);
            }
            Some(Cmd::CreateBranch(name, from, tx)) => {
                let _ = tx.send(self.create_branch(name, from).await);
            }
            Some(Cmd::UserTool(call, tx)) => {
                let reply = self.user_tool(call).await;
                let _ = tx.send(reply);
            }
            Some(Cmd::RetryGroupMerge(group, tx)) if self.already_merged(&group) => {
                let _ = tx.send(Ok(Value::Null)); // another client's retry merged it
            }
            Some(Cmd::RetryGroupMerge(group, tx)) => {
                let reply = self
                    .execute(DomainCommand::RetryGroupMerge { group })
                    .await
                    .unwrap_or_else(|| Err(ToolError::internal("the retry produced no reply")));
                let _ = tx.send(reply);
            }
            Some(Cmd::Snapshot(tx)) => {
                let _ = tx.send(self.snapshot().await);
            }
            Some(Cmd::Activity(tx)) => {
                let _ = tx.send(self.activity().await);
            }
            Some(Cmd::Shutdown(done)) => {
                self.shutdown().await;
                let _ = done.send(());
                return false;
            }
            None => {
                self.shutdown().await;
                return false;
            }
        }
        true
    }

    /// The state as of the last durable event (everything buffered is published
    /// first, so the sessions read from the store match the same `seq`).
    async fn snapshot(&mut self) -> Result<Snapshot, StoreError> {
        self.publisher.flush().await;
        let store = self.publisher.store();
        let sessions = store.sessions(&self.cfg.project).await?;
        let chats = store.chats(&self.cfg.project).await?;
        let mut starting: Vec<String> = self.sessions.starting.keys().cloned().collect();
        starting.sort();
        Ok(Snapshot {
            seq: self.publisher.seq(),
            state: self.state.clone(),
            sessions,
            chats,
            starting,
        })
    }

    /// Whether work is on (a live agent session is in a turn, or a task is
    /// running or merging: the project is "running" in the sidebar) and whether
    /// one of its chats has an orchestrator session up (the project is "awake").
    /// Read like [`Driver::snapshot`], so both match the same `seq`.
    async fn activity(&mut self) -> Result<(bool, bool), StoreError> {
        self.publisher.flush().await;
        let sessions = self.publisher.store().sessions(&self.cfg.project).await?;
        let running = self.state.has_working_task()
            || sessions
                .iter()
                .any(|s| s.status == SessionStatus::Live && s.turn_running);
        let awake = sessions
            .iter()
            .any(|s| s.status == SessionStatus::Live && chat_of_session(&s.session_key).is_some());
        Ok((running, awake))
    }

    /// Runs an orchestrator tool for the user, records it like an agent's tool
    /// call, and tells the orchestrator what the user did.
    async fn user_tool(&mut self, call: ToolCall) -> Reply {
        if self.already_cancelled(&call) {
            return Ok(Value::Null); // another client cancelled it first
        }
        let binding = SessionBinding {
            session: USER_SESSION.into(),
            role: crate::domain::Role::Orchestrator,
            project: self.cfg.project.clone(),
            chat: None,
            group: None,
            task: None,
            step: None,
        };
        let note = user_action_note(&call);
        let chat = self.chat_of_call(&call);
        let record_call = serde_json::to_value(&call).unwrap_or_default();
        let reply = self.tool_reply(binding.clone(), call).await;
        self.emit.send(ApiEventBody::ToolCalled {
            record: ToolCallRecord {
                binding,
                tool: record_call["tool"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
                args: record_call["args"].clone(),
                result: reply.clone(),
            },
        });
        if let (Ok(_), Some(text), Some(chat)) = (&reply, note, chat) {
            self.execute(DomainCommand::UserMessage { chat, text })
                .await;
        }
        reply
    }

    async fn shutdown(&mut self) {
        self.return_messages("Yhtye is shutting down.").await;
        for body in self.transcript.close_all() {
            self.emit.send(body);
        }
        self.sessions.shutdown().await;
        self.publisher.flush().await;
    }

    async fn on_tool(&mut self, req: ToolRequest) {
        let ToolRequest {
            binding,
            call,
            reply,
        } = req;
        if let ToolCall::MessageTask(args) = call {
            // Answered when the message is delivered or handed back, not now.
            return self.message_task(binding, args, reply).await;
        }
        let answer = self.tool_reply(binding, call).await;
        // The caller may have gone away (e.g. its HTTP request was dropped).
        let _ = reply.send(answer);
    }

    async fn tool_reply(&mut self, binding: SessionBinding, call: ToolCall) -> Reply {
        let cmd = match call {
            ToolCall::CreateGroup(args) => {
                let chat = binding.chat.clone().ok_or_else(|| {
                    ToolError::forbidden("create_group needs an orchestrator chat")
                })?;
                let base_branch = self.checked_out_branch(&chat).await?;
                let taken_in_git = self.name_in_git(&args.name, None).await?;
                DomainCommand::CreateGroup {
                    chat,
                    args,
                    base_branch,
                    taken_in_git,
                }
            }
            ToolCall::CreateTask(args) => {
                let settings = self.cfg.agents.available_settings(Some(&self.cfg.project));
                let args = choose_agents(&settings, args)?;
                let taken_in_git = self.name_in_git(&args.group_id, Some(&args.name)).await?;
                DomainCommand::CreateTask {
                    binding,
                    args,
                    taken_in_git,
                }
            }
            ToolCall::FinishGroup(args) => {
                if let (Some(into), Some(chat)) = (&args.into, &binding.chat) {
                    self.check_into(chat, into).await?;
                }
                DomainCommand::Tool {
                    binding,
                    call: ToolCall::FinishGroup(args),
                }
            }
            call => DomainCommand::Tool { binding, call },
        };
        let status = matches!(
            &cmd,
            DomainCommand::Tool {
                call: ToolCall::GetStatus(_),
                ..
            }
        );
        let reply = self
            .execute(cmd)
            .await
            .unwrap_or_else(|| Err(ToolError::internal("the command produced no reply")));
        match reply {
            Ok(Value::Object(mut body)) if status => {
                let settings = self.cfg.agents.available_settings(Some(&self.cfg.project));
                body.insert("agents".into(), agents_status(&settings));
                Ok(Value::Object(body))
            }
            other => other,
        }
    }

    /// What in git already uses the name of a new group (`task: None`) or of a
    /// new task of the existing group `group` (which may have an older numbered
    /// id). Only asked for a valid name: the state machine refuses the others.
    async fn name_in_git(
        &self,
        group: &str,
        task: Option<&str>,
    ) -> Result<Option<String>, ToolError> {
        let looked_up = match task {
            None => crate::domain::is_valid_name(group),
            Some(task) => crate::domain::is_valid_name(task) && self.state.group(group).is_some(),
        };
        if !looked_up {
            return Ok(None);
        }
        self.git
            .name_in_git(group, task)
            .await
            .map_err(|e| ToolError::internal(format!("could not look up the name in git: {e}")))
    }

    /// Applies `cmd` and everything that follows from it (git results), and
    /// returns the last tool reply produced along the way.
    pub(super) async fn execute(&mut self, cmd: DomainCommand) -> Option<Reply> {
        let mut reply = None;
        let mut queue = VecDeque::from([cmd]);
        let mut first = true;
        while let Some(cmd) = queue.pop_front() {
            match decide(&self.state, cmd) {
                Ok((next, t)) => {
                    // Store first: effects run only for a transition that is on disk.
                    if let Err(e) = self.publisher.commit(&self.state, &next, t.events).await {
                        // Also for a follow-up (e.g. a git result): the call did not
                        // fully take effect, so it must not be reported as a success.
                        tracing::error!("storing a transition failed; it was dropped: {e}");
                        reply = Some(Err(ToolError::internal(format!(
                            "Yhtye could not save the change: {e}"
                        ))));
                        first = false;
                        continue;
                    }
                    self.state = next;
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
                let pick = self.pick(&agent);
                self.sessions.prompt(binding, cwd, prompt, pick);
            }
            Effect::ResumeStep {
                agent,
                group,
                prompt,
                fallback,
                workdir,
            } => {
                let fallback = self
                    .with_changes(fallback, workdir.as_deref(), &group)
                    .await;
                let cwd = workdir.unwrap_or_else(|| self.cfg.project_dir.clone());
                let binding = self.binding(&agent, Some(group));
                let pick = self.pick(&agent);
                self.sessions
                    .resume_step(binding, cwd, prompt, fallback, pick);
            }
            Effect::PromptAgent { agent, text } => {
                let t = self.state.task(&agent.task);
                let group = t.map(|t| t.group.clone());
                let cwd = t
                    .and_then(|t| t.workdir.clone())
                    .unwrap_or_else(|| self.cfg.project_dir.clone());
                let binding = self.binding(&agent, group);
                let pick = self.pick(&agent);
                self.sessions.prompt(binding, cwd, text, pick);
            }
            Effect::StopAgent { agent } => self.sessions.stop(&session_key(&agent)),
            Effect::StopTaskAgents { task } => self.sessions.stop_task(&task),
            Effect::Git(op) => {
                let op = self.with_commit_agents(op).await;
                let result = self.git.run(&op).await;
                return Some(DomainCommand::GitDone { op, result });
            }
            Effect::DeliverMessage { agent, text } => return self.deliver_message(agent, text),
            Effect::WakeOrchestrator { .. } => {} // the inbox is flushed after every loop turn
        }
        None
    }

    /// A restart fallback prompt plus what is already in the working directory
    /// (`orchestration-model.md` §10), so a new session does not redo the work.
    async fn with_changes(&self, fallback: String, workdir: Option<&Path>, group: &str) -> String {
        let (Some(dir), Some(g)) = (workdir, self.state.group(group)) else {
            return fallback;
        };
        match self.git.workspace_changes(dir, &g.group_branch).await {
            Ok(changes) if !changes.trim().is_empty() => {
                fallback + &workspace_changes_note(&changes)
            }
            Ok(_) => fallback,
            Err(e) => {
                tracing::warn!("could not read the changes in {}: {e}", dir.display());
                fallback
            }
        }
    }

    /// The selection role and the task's override for the session of `agent`
    /// (`core-design.md` §15.4).
    fn pick(&self, agent: &AgentRef) -> AgentPick {
        let task = self.state.task(&agent.task);
        let over = task.and_then(|t| match agent.role {
            Role::Reviewer => t.review_agent.clone(),
            Role::Implementer | Role::Orchestrator => t.agent.clone(),
        });
        AgentPick {
            role: AgentRole::of_session(agent.role, task.map(|t| t.kind)),
            over,
        }
    }

    fn binding(&self, agent: &AgentRef, group: Option<String>) -> SessionBinding {
        SessionBinding {
            session: session_key(agent),
            role: agent.role,
            project: self.cfg.project.clone(),
            chat: None,
            group,
            task: Some(agent.task.clone()),
            step: Some(agent.step),
        }
    }

    async fn on_spawned(&mut self, spawned: Spawned) {
        let key = spawned.binding.session.clone();
        let failed = self.sessions.on_spawned(spawned);
        self.orchestrator_spawned(&key).await;
        let Some((binding, error)) = failed else {
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
        if let Some(block) = self.transcript.observe(&key, &event) {
            self.emit.send(block);
        }
        self.emit.send(ApiEventBody::Agent {
            session: key.clone(),
            event: event.clone(),
        });
        match event {
            AgentEvent::TurnEnded(result) => {
                self.sessions.turn_ended(&key);
                let binding = self.sessions.binding(&key);
                let agent = binding.and_then(agent_ref);
                let chat = binding.and_then(|b| b.chat.clone());
                let mut outcome = turn_outcome(result);
                if let Some(agent) = &agent {
                    // A message waiting for this turn end is queued here, before
                    // `prompt_queued` is read.
                    outcome = self.message_at_turn_end(agent, outcome).await;
                }
                let prompt_queued = self.sessions.has_queued(&key);
                if let Some(agent) = agent {
                    self.execute(DomainCommand::TurnEnded {
                        agent,
                        outcome,
                        prompt_queued,
                    })
                    .await;
                } else if let Some(chat) = chat {
                    self.orchestrator_turn_ended(chat, outcome, prompt_queued)
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
}

/// What the orchestrator is told when the user acts directly from the UI.
fn user_action_note(call: &ToolCall) -> Option<String> {
    match call {
        ToolCall::CancelTask(a) => Some(format!(
            "(Yhtye note: the user cancelled task {} from the Yhtye UI. Reason: {})",
            a.task_id, a.reason
        )),
        ToolCall::CancelGroup(a) => Some(format!(
            "(Yhtye note: the user cancelled group {} from the Yhtye UI. Reason: {})",
            a.group_id, a.reason
        )),
        _ => None,
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
