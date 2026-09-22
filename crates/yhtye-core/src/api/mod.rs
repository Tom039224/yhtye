//! The UI-facing API (`core-design.md` §2). Stage 3a defines the event stream and
//! snapshot; commands (`ApiCommand`) and TypeScript generation come with the
//! Tauri / WS integration.

use serde::{Deserialize, Serialize};

use crate::acp::{AgentEvent, AgentOutput};
use crate::domain::{DomainEvent, Role, State};
use crate::mcp::ToolCallRecord;

/// One event of a project.
///
/// Durable events (`live == false`) are stored in the event log and numbered
/// without gaps from 1; `seq` is their row in the log. Live events (streaming
/// text chunks and usage updates, see [`ApiEventBody::is_live`]) are not stored:
/// they carry the `seq` of the last durable event and never advance it. A text
/// block streamed as live chunks is stored once, coalesced, as
/// [`ApiEventBody::AgentText`] when the block ends.
///
/// Sync rule for clients: take a [`Snapshot`], then apply only durable events with
/// `seq > snapshot.seq`; on a gap between durable events, take a new snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct ApiEvent {
    pub seq: u64,
    /// Milliseconds since the Unix epoch.
    pub ts_ms: u64,
    pub project: String,
    pub live: bool,
    pub body: ApiEventBody,
}

/// Kind of a coalesced text block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    Message,
    Thought,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiEventBody {
    /// A state change (apply it with the same reducer as [`State::apply`]).
    Domain {
        event: DomainEvent,
    },
    SessionStarted {
        session: String,
        role: Role,
        task: Option<String>,
        pid: Option<u32>,
        acp_session_id: String,
        /// Restored with `session/load` (after a restart).
        resumed: bool,
    },
    SessionFailed {
        session: String,
        error: String,
    },
    /// The session was shut down (or its process exited) and its token revoked.
    SessionStopped {
        session: String,
        /// Stopped because Yhtye shut down; it is restored on the next start.
        suspended: bool,
    },
    /// On startup: the session was running (or suspended) when Yhtye stopped.
    /// It is restored with `session/load` when its task resumes.
    SessionInterrupted {
        session: String,
    },
    /// Agent output and lifecycle (stderr lines are not forwarded).
    Agent {
        session: String,
        event: AgentEvent,
    },
    /// A complete message / thought block of an agent (its chunks were live events).
    AgentText {
        session: String,
        kind: TextKind,
        text: String,
    },
    ToolCalled {
        record: ToolCallRecord,
    },
    /// A prompt was sent to a session (for the orchestrator: an inbox batch).
    Prompted {
        session: String,
        text: String,
    },
}

impl ApiEventBody {
    /// Live events are streamed to clients but not stored (see [`ApiEvent`]).
    #[must_use]
    pub fn is_live(&self) -> bool {
        matches!(
            self,
            Self::Agent {
                event: AgentEvent::Output(
                    AgentOutput::MessageChunk(_)
                        | AgentOutput::ThoughtChunk(_)
                        | AgentOutput::UserMessageChunk(_)
                        | AgentOutput::Usage(_)
                ),
                ..
            }
        )
    }

    /// The session key the event belongs to, if any.
    #[must_use]
    pub fn session(&self) -> Option<&str> {
        match self {
            Self::Domain { .. } => None,
            Self::SessionStarted { session, .. }
            | Self::SessionFailed { session, .. }
            | Self::SessionStopped { session, .. }
            | Self::SessionInterrupted { session }
            | Self::Agent { session, .. }
            | Self::AgentText { session, .. }
            | Self::Prompted { session, .. } => Some(session),
            Self::ToolCalled { record } => Some(&record.binding.session),
        }
    }
}

/// The state of a project as of event `seq`.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub seq: u64,
    pub state: State,
}
