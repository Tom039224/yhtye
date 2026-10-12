//! `message_task` (`mcp-tools.md` §3, `orchestration-model.md` §7.3): a message
//! from the orchestrator to the agent working on a running task's current step.
//!
//! The state machine decides whether a message may be sent at all and logs it;
//! the runtime delivers it (steering, or an interrupt followed by a prompt;
//! `core-design.md` §8) and settles it with `MessageResolved`, which is the
//! answer of the tool call.

use serde_json::{Value, json};

use super::command::{AgentRef, Effect};
use super::event::DomainEvent;
use super::flow::step_kind_label;
use super::machine::{Tx, non_empty};
use super::state::{StepStatus, TaskStatus};
use super::types::{MessageOutcome, MessageVia, Role, ToolError};
use crate::mcp::SessionBinding;
use crate::mcp::tools::MessageTaskArgs;

/// What the orchestrator can do instead when it cannot message a task that is
/// `status` (completes "task T-1 is checkpoint; ...").
#[must_use]
pub fn unavailable_hint(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Running => "its agent is working on it",
        TaskStatus::Pending | TaskStatus::AwaitingInstruction => {
            "it has no agent yet: set_instruction gives it its instruction, and modify_steps \
             changes its steps"
        }
        TaskStatus::Checkpoint => {
            "it is waiting at a checkpoint: resolve_checkpoint passes a `note` to the agent of \
             the next step"
        }
        TaskStatus::Handling => {
            "it is waiting for your answer to a help request: answer_help sends your `reply` to \
             the agent"
        }
        TaskStatus::Interrupted => {
            "Yhtye was restarted and resumes it by itself; send the message again in a moment"
        }
        TaskStatus::Merging => "its work is being merged; there is no agent to message",
        TaskStatus::Done | TaskStatus::Cancelled => "it has ended; create_task adds further work",
    }
}

/// Why a message is not delivered once its step has reported done: what the
/// orchestrator can do instead (`step` is the step's index).
#[must_use]
pub fn reported_detail(role: Role, step: usize) -> String {
    format!(
        "The {role} already reported step {} done (report_step_done) and is only finishing its \
         turn, so the message was not delivered. For more work, add a step after it with \
         modify_steps (for example another implement) or create a new task.",
        step + 1
    )
}

/// The tool reply (`message_task`'s return value).
fn message_reply(
    task: &str,
    step: usize,
    outcome: MessageOutcome,
    via: Option<MessageVia>,
    detail: &str,
) -> Value {
    json!({ "task_id": task, "step": step, "outcome": outcome, "via": via, "detail": detail })
}

impl Tx {
    /// `message_task`: refuses what the state rules out, hands a message to a
    /// step that is running to the runtime, and returns one to a step that has
    /// already reported (or not started) at once.
    pub(super) fn message_task(
        &mut self,
        binding: &SessionBinding,
        a: MessageTaskArgs,
        pending: bool,
    ) -> Result<(), ToolError> {
        self.forbid_foreign(binding, self.state.chat_of_task(&a.task_id))?;
        non_empty("text", &a.text)?;
        let t = self.task(&a.task_id)?;
        if t.status != TaskStatus::Running {
            return Err(ToolError::invalid_state(format!(
                "task {} is {}; {}",
                t.id,
                t.status.as_str(),
                unavailable_hint(t.status)
            )));
        }
        if pending {
            return Err(ToolError::conflict(format!(
                "a message to task {} is still being delivered; wait for its reply before \
                 sending another",
                t.id
            )));
        }
        let (task, step) = (t.id.clone(), t.current);
        let Some(current) = t.current_step() else {
            return Err(ToolError::invalid_state(format!(
                "task {task} has no current step"
            )));
        };
        let Some(role) = current.kind.role() else {
            return Err(ToolError::invalid_state(format!(
                "task {task} is on its `{}` step, which Yhtye runs itself; there is no agent to \
                 message",
                step_kind_label(current.kind)
            )));
        };
        let early = match current.status {
            StepStatus::Pending => Some(
                "The step has not started yet (the task's workspace is being prepared). Send \
                 the message again in a moment, or put it in the step's instruction with \
                 modify_steps."
                    .to_string(),
            ),
            StepStatus::Done => Some(reported_detail(role, step)),
            StepStatus::Running => None,
        };
        self.emit(DomainEvent::TaskMessageSent {
            task: task.clone(),
            step,
            text: a.text.clone(),
        });
        match early {
            Some(detail) => {
                self.message_resolved(&task, step, MessageOutcome::Returned, None, &detail);
            }
            None => self.effect(Effect::DeliverMessage {
                agent: AgentRef { task, role, step },
                text: a.text,
            }),
        }
        Ok(())
    }

    /// The runtime settled the message to the agent of `task`'s step `step`:
    /// logs the result and answers the tool call.
    pub(super) fn message_resolved(
        &mut self,
        task: &str,
        step: usize,
        outcome: MessageOutcome,
        via: Option<MessageVia>,
        detail: &str,
    ) {
        self.emit(DomainEvent::TaskMessageResolved {
            task: task.to_string(),
            step,
            outcome,
            via,
            detail: detail.to_string(),
        });
        self.reply(Ok(message_reply(task, step, outcome, via, detail)));
    }
}
