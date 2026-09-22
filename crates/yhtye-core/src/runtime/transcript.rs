//! Coalesces streamed message / thought chunks into whole text blocks.
//!
//! Chunks are forwarded to the UI as live events (not stored). A block ends when
//! the same session reports anything else (a chunk of the other kind, a tool
//! call, the end of the turn, its exit) or the runtime shuts down; the whole
//! block is then published once as a durable [`ApiEventBody::AgentText`].

use std::collections::HashMap;

use crate::acp::{AgentEvent, AgentOutput};
use crate::api::{ApiEventBody, TextKind};

#[derive(Default)]
pub(super) struct Transcript {
    open: HashMap<String, (TextKind, String)>,
}

impl Transcript {
    /// Records `event` of `session`. Returns the block it closed, if any.
    pub(super) fn observe(&mut self, session: &str, event: &AgentEvent) -> Option<ApiEventBody> {
        match text_chunk(event) {
            Some((kind, text)) => {
                let closed = match self.open.get(session) {
                    Some((open_kind, _)) if *open_kind != kind => self.close(session),
                    _ => None,
                };
                self.open
                    .entry(session.to_string())
                    .or_insert_with(|| (kind, String::new()))
                    .1
                    .push_str(text);
                closed
            }
            // Other chunk types (user echo, usage) do not end a block.
            None if event_is_live_only(event) => None,
            None => self.close(session),
        }
    }

    /// Ends the open block of `session`, if any.
    pub(super) fn close(&mut self, session: &str) -> Option<ApiEventBody> {
        let (kind, text) = self.open.remove(session)?;
        (!text.is_empty()).then(|| ApiEventBody::AgentText {
            session: session.to_string(),
            kind,
            text,
        })
    }

    /// Ends every open block (on shutdown).
    pub(super) fn close_all(&mut self) -> Vec<ApiEventBody> {
        let sessions: Vec<String> = self.open.keys().cloned().collect();
        sessions.iter().filter_map(|s| self.close(s)).collect()
    }
}

fn text_chunk(event: &AgentEvent) -> Option<(TextKind, &str)> {
    let AgentEvent::Output(output) = event else {
        return None;
    };
    let kind = match output {
        AgentOutput::MessageChunk(_) => TextKind::Message,
        AgentOutput::ThoughtChunk(_) => TextKind::Thought,
        _ => return None,
    };
    output.chunk_text().map(|t| (kind, t))
}

fn event_is_live_only(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::Output(
            AgentOutput::UserMessageChunk(_)
                | AgentOutput::Usage(_)
                | AgentOutput::MessageChunk(_)
                | AgentOutput::ThoughtChunk(_)
        ) | AgentEvent::Stderr(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::schema::{ContentBlock, ContentChunk, StopReason, TextContent};

    fn chunk(text: &str) -> AgentEvent {
        AgentEvent::Output(AgentOutput::MessageChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new(text)),
        )))
    }

    fn thought(text: &str) -> AgentEvent {
        AgentEvent::Output(AgentOutput::ThoughtChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new(text)),
        )))
    }

    fn text_of(body: Option<ApiEventBody>) -> Option<(TextKind, String)> {
        match body? {
            ApiEventBody::AgentText { kind, text, .. } => Some((kind, text)),
            _ => None,
        }
    }

    #[test]
    fn chunks_become_one_block_at_turn_end() {
        let mut t = Transcript::default();
        assert!(t.observe("s", &chunk("hel")).is_none());
        assert!(t.observe("s", &chunk("lo")).is_none());
        let end = AgentEvent::TurnEnded(Ok(StopReason::EndTurn));
        assert_eq!(
            text_of(t.observe("s", &end)),
            Some((TextKind::Message, "hello".into()))
        );
        assert!(t.observe("s", &end).is_none());
    }

    #[test]
    fn a_kind_change_closes_the_block_and_sessions_are_separate() {
        let mut t = Transcript::default();
        t.observe("a", &thought("think"));
        t.observe("b", &chunk("other"));
        assert_eq!(
            text_of(t.observe("a", &chunk("say"))),
            Some((TextKind::Thought, "think".into()))
        );
        let mut rest: Vec<_> = t
            .close_all()
            .into_iter()
            .filter_map(|b| text_of(Some(b)))
            .collect();
        rest.sort_by(|x, y| x.1.cmp(&y.1));
        assert_eq!(
            rest,
            vec![
                (TextKind::Message, "other".into()),
                (TextKind::Message, "say".into())
            ]
        );
    }
}
