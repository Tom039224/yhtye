//! Inbox messages from Yhtye to the orchestrator (`mcp-tools.md` §5).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum InboxKind {
    UserMessage,
    CheckpointReached,
    HelpRaised,
    InstructionNeeded,
    GroupSettled,
    MergeResult,
    /// Yhtye restarted and the orchestrator missed something (Stage 3b).
    Restarted,
}

impl InboxKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserMessage => "user_message",
            Self::CheckpointReached => "checkpoint_reached",
            Self::HelpRaised => "help_raised",
            Self::InstructionNeeded => "instruction_needed",
            Self::GroupSettled => "group_settled",
            Self::MergeResult => "merge_result",
            Self::Restarted => "restarted",
        }
    }
}

/// One reason to wake the orchestrator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InboxItem {
    pub kind: InboxKind,
    pub attrs: Vec<(String, String)>,
    pub body: String,
}

impl InboxItem {
    #[must_use]
    pub fn new(kind: InboxKind, attrs: &[(&str, &str)], body: impl Into<String>) -> Self {
        Self {
            kind,
            attrs: attrs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            body: body.into(),
        }
    }

    #[must_use]
    pub fn user_message(text: impl Into<String>) -> Self {
        Self::new(InboxKind::UserMessage, &[], text)
    }

    /// `[yhtye:<type>] key=value ...` followed by the body on the next lines.
    #[must_use]
    pub fn render(&self) -> String {
        let mut head = format!("[yhtye:{}]", self.kind.as_str());
        for (k, v) in &self.attrs {
            head.push_str(&format!(" {k}={v}"));
        }
        if self.body.is_empty() {
            head
        } else {
            format!("{head}\n{}", self.body.trim_end())
        }
    }
}

/// All pending items as one prompt (blocks separated by a blank line).
#[must_use]
pub fn render_batch(items: &[InboxItem]) -> String {
    items
        .iter()
        .map(InboxItem::render)
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_header_attrs_and_body() {
        let item = InboxItem::new(
            InboxKind::HelpRaised,
            &[("help_id", "H-3"), ("task", "T-110"), ("kind", "blocked")],
            "cannot find the file\n",
        );
        assert_eq!(
            item.render(),
            "[yhtye:help_raised] help_id=H-3 task=T-110 kind=blocked\ncannot find the file"
        );
    }

    #[test]
    fn batches_are_separated_by_blank_lines() {
        let items = [
            InboxItem::user_message("hi"),
            InboxItem::new(InboxKind::GroupSettled, &[("group", "G-1")], ""),
        ];
        assert_eq!(
            render_batch(&items),
            "[yhtye:user_message]\nhi\n\n[yhtye:group_settled] group=G-1"
        );
    }
}
