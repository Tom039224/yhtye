//! User actions that another client already carried out (T-65,
//! `core-design.md` §19). Two clients (the PC and a phone) may send the same
//! action at nearly the same time; the loop runs them one after the other, and
//! the second finds its target state already reached. That one is answered as
//! accepted, doing nothing (no tool call is recorded and the orchestrator is not
//! told twice), instead of failing with `invalid_state` / `not_found`.

use super::driver::Driver;
use crate::domain::{GroupStatus, TaskStatus, chat_number};
use crate::mcp::ToolCall;

impl Driver {
    /// A user's `cancel_task` / `cancel_group` whose target is already cancelled.
    pub(super) fn already_cancelled(&self, call: &ToolCall) -> bool {
        match call {
            ToolCall::CancelTask(a) => self
                .state
                .task(&a.task_id)
                .is_some_and(|t| t.status == TaskStatus::Cancelled),
            ToolCall::CancelGroup(a) => self
                .state
                .group(&a.group_id)
                .is_some_and(|g| g.status == GroupStatus::Cancelled),
            _ => false,
        }
    }

    /// A retried merge of a group that is merged already (another retry won).
    pub(super) fn already_merged(&self, group: &str) -> bool {
        self.state
            .group(group)
            .is_some_and(|g| g.status == GroupStatus::Done)
    }

    /// A chat that existed and was deleted: not in the state, but numbered no
    /// higher than the last chat created (ids are not reused). A chat id that
    /// was never given out stays `not_found`.
    pub(super) fn already_deleted(&self, chat: &str) -> bool {
        self.state.chat(chat).is_none()
            && chat_number(chat).is_some_and(|n| n <= self.state.counters.chats)
    }

    /// A `/compact` asked for while the same chat's compaction (another
    /// client's) is still running.
    pub(super) fn already_compacting(&self, chat: &str) -> bool {
        self.orchestrators.compacting.contains(chat)
    }
}
