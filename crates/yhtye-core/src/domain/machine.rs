//! Entry point of the state machine: [`decide`] handles one [`DomainCommand`]
//! against a state and returns the new state plus a [`Transition`]
//! (events, effects, tool reply). Nothing is changed when it returns an error.

use serde_json::Value;

use super::command::{DomainCommand, Effect, Transition};
use super::event::DomainEvent;
use super::inbox::InboxItem;
use super::state::{InboxEntry, State, Task};
use super::types::ToolError;

/// A command being decided: a working copy of the state plus what happened so far.
pub(super) struct Tx {
    pub(super) state: State,
    events: Vec<DomainEvent>,
    effects: Vec<Effect>,
    reply: Option<Result<Value, ToolError>>,
}

impl Tx {
    fn new(state: State) -> Self {
        Self {
            state,
            events: Vec::new(),
            effects: Vec::new(),
            reply: None,
        }
    }

    /// Records an event and applies it to the working state.
    pub(super) fn emit(&mut self, event: DomainEvent) {
        self.state.apply(&event);
        self.events.push(event);
    }

    pub(super) fn effect(&mut self, effect: Effect) {
        self.effects.push(effect);
    }

    pub(super) fn reply(&mut self, reply: Result<Value, ToolError>) {
        self.reply = Some(reply);
    }

    pub(super) fn task(&self, id: &str) -> Result<&Task, ToolError> {
        self.state
            .task(id)
            .ok_or_else(|| ToolError::not_found(format!("no task {id}")))
    }

    /// Queues an inbox item for the orchestrator of `chat` and asks the runtime
    /// to wake it.
    pub(super) fn queue_inbox(&mut self, chat: &str, item: InboxItem) {
        let id = self.state.counters.inbox + 1;
        self.emit(DomainEvent::InboxQueued {
            entry: InboxEntry {
                id,
                chat: chat.to_string(),
                item,
            },
        });
        self.effect(Effect::WakeOrchestrator {
            chat: chat.to_string(),
        });
    }

    /// Queues an item for the chat that created `group`.
    pub(super) fn queue_inbox_group(&mut self, group: &str, item: InboxItem) {
        match self.state.chat_of_group(group).map(str::to_string) {
            Some(chat) => self.queue_inbox(&chat, item),
            None => tracing::error!("group {group} has no chat; dropping {:?}", item.kind),
        }
    }

    /// Queues an item for the chat of the group `task` belongs to.
    pub(super) fn queue_inbox_task(&mut self, task: &str, item: InboxItem) {
        match self.state.chat_of_task(task).map(str::to_string) {
            Some(chat) => self.queue_inbox(&chat, item),
            None => tracing::error!("task {task} has no chat; dropping {:?}", item.kind),
        }
    }

    fn finish(self) -> (State, Transition) {
        let transition = Transition {
            events: self.events,
            effects: self.effects,
            reply: self.reply,
        };
        (self.state, transition)
    }
}

/// Handles `cmd` against `state`. Pure: no I/O, `state` is not modified.
pub fn decide(state: &State, cmd: DomainCommand) -> Result<(State, Transition), ToolError> {
    let mut tx = Tx::new(state.clone());
    match cmd {
        DomainCommand::CreateChat { worktree } => tx.create_chat(worktree)?,
        DomainCommand::CreateGroup {
            chat,
            args,
            base_branch,
            taken,
        } => tx.create_group(&chat, args, base_branch, taken)?,
        DomainCommand::Tool { binding, call } => tx.tool(&binding, call)?,
        DomainCommand::UserMessage { chat, text } => tx.user_message(&chat, text)?,
        DomainCommand::OrchestratorTurnEnded {
            chat,
            outcome,
            prompt_queued,
        } => tx.orchestrator_turn_ended(&chat, &outcome, prompt_queued),
        DomainCommand::TurnEnded {
            agent,
            outcome,
            prompt_queued,
        } => tx.turn_ended(&agent, &outcome, prompt_queued),
        DomainCommand::AgentExited { agent, detail } => tx.agent_exited(&agent, &detail),
        DomainCommand::AgentStartFailed { agent, error } => tx.agent_start_failed(&agent, &error),
        DomainCommand::GitDone { op, result } => tx.git_done(op, result),
        DomainCommand::InboxDelivered { chat, up_to } => {
            tx.emit(DomainEvent::InboxDelivered { chat, up_to });
        }
        DomainCommand::Restart { orchestrators } => tx.restart(&orchestrators),
        DomainCommand::TellOrchestrator { chat, resume } => tx.tell_orchestrator(&chat, resume),
        DomainCommand::ResumeTask { task } => tx.resume_task(&task),
        DomainCommand::RetryGroupMerge { group } => tx.retry_group_merge(&group)?,
    }
    Ok(tx.finish())
}

/// Owns the current state and applies commands to it.
#[derive(Debug, Clone)]
pub struct Machine {
    state: State,
}

impl Machine {
    #[must_use]
    pub fn new(state: State) -> Self {
        Self { state }
    }

    #[must_use]
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Applies `cmd`; on error the state is unchanged.
    pub fn handle(&mut self, cmd: DomainCommand) -> Result<Transition, ToolError> {
        let (next, transition) = decide(&self.state, cmd)?;
        self.state = next;
        Ok(transition)
    }
}

/// `field` must contain something other than whitespace.
pub(super) fn non_empty(field: &str, value: &str) -> Result<(), ToolError> {
    if value.trim().is_empty() {
        return Err(ToolError::invalid_argument(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}
