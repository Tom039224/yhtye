//! A prompt waiting for its session: plain text, or an orchestrator's inbox
//! batch whose attachments are read and delivered when it is sent (with the
//! capabilities of the session that gets it, which a session still starting
//! does not have yet).

use crate::attachments::prompt::{BuiltPrompt, PromptEnv, build_inbox_prompt};
use crate::domain::InboxEntry;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Prompt {
    Text(String),
    /// Inbox entries of one chat, oldest first.
    Inbox(Vec<InboxEntry>),
}

impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl Prompt {
    /// The text blocks and attachment blocks to send.
    pub(super) fn build(&self, env: PromptEnv<'_>) -> BuiltPrompt {
        match self {
            Self::Text(text) => BuiltPrompt::text_only(text.clone()),
            Self::Inbox(entries) => build_inbox_prompt(entries, Some(env)),
        }
    }
}
