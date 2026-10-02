//! Chats (`orchestration-model.md` §2.0, Stage 8): creating one and the user's
//! messages to its orchestrator.

use std::path::PathBuf;

use serde_json::json;

use super::event::DomainEvent;
use super::inbox::InboxItem;
use super::machine::{Tx, non_empty};
use super::state::Chat;
use super::types::ToolError;

/// Longest chat title, in characters.
pub const MAX_TITLE_CHARS: usize = 40;

/// Prefix of Yhtye's internal branches (`yhtye/<groupId>`); a chat cannot work on one.
pub const INTERNAL_BRANCH_PREFIX: &str = "yhtye/";

/// Prefix of the session key of a chat's orchestrator (`orchestrator:<chatId>`).
const ORCHESTRATOR_SESSION_PREFIX: &str = "orchestrator:";

/// Session key of the orchestrator of `chat` (`orchestrator:C-1`).
#[must_use]
pub fn orchestrator_session(chat: &str) -> String {
    format!("{ORCHESTRATOR_SESSION_PREFIX}{chat}")
}

/// The chat whose orchestrator has session key `session`.
#[must_use]
pub fn chat_of_session(session: &str) -> Option<&str> {
    session.strip_prefix(ORCHESTRATOR_SESSION_PREFIX)
}

impl Tx {
    pub(super) fn create_chat(&mut self, worktree: PathBuf) -> Result<(), ToolError> {
        if worktree.as_os_str().is_empty() {
            return Err(ToolError::invalid_argument("a chat needs a worktree"));
        }
        let id = format!("C-{}", self.state.counters.chats + 1);
        let chat = Chat {
            id,
            worktree,
            title: None,
        };
        self.reply(Ok(json!({ "chat": chat })));
        self.emit(DomainEvent::ChatCreated { chat });
        Ok(())
    }

    /// The user's message: it is titled from the chat's first message, then queued.
    pub(super) fn user_message(&mut self, chat: &str, text: String) -> Result<(), ToolError> {
        let c = self
            .state
            .chat(chat)
            .ok_or_else(|| ToolError::not_found(format!("no chat {chat}")))?;
        if c.title.is_none() {
            let title = title_of(&text);
            if !title.is_empty() {
                self.emit(DomainEvent::ChatTitled {
                    chat: chat.to_string(),
                    title,
                });
            }
        }
        self.queue_inbox(chat, InboxItem::user_message(text));
        Ok(())
    }
}

/// A branch a chat works on (or a group merges into) must be named and not
/// one of Yhtye's internal branches.
pub fn check_target_branch(branch: &str) -> Result<(), ToolError> {
    non_empty("branch", branch)?;
    if branch.starts_with(INTERNAL_BRANCH_PREFIX) {
        return Err(ToolError::invalid_argument(format!(
            "{branch} is one of Yhtye's internal branches; pick another branch"
        )));
    }
    Ok(())
}

/// The first `MAX_TITLE_CHARS` characters, newlines as spaces.
fn title_of(text: &str) -> String {
    let flat: String = text
        .trim()
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .take(MAX_TITLE_CHARS)
        .collect();
    flat.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_flattened_and_cut() {
        assert_eq!(title_of("  add a\nline  "), "add a line");
        let long = "あ".repeat(100);
        assert_eq!(title_of(&long).chars().count(), MAX_TITLE_CHARS);
    }
}
