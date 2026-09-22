//! The UI-facing API (`core-design.md` §2). Stage 3a defines the event stream and
//! snapshot; commands (`ApiCommand`) and TypeScript generation come with the
//! Tauri / WS integration.

use serde::Serialize;

use crate::acp::AgentEvent;
use crate::domain::{DomainEvent, Role, State};
use crate::mcp::ToolCallRecord;

/// One event of a project, numbered without gaps from 1.
///
/// Sync rule for clients: take a [`Snapshot`], then apply only events with
/// `seq > snapshot.seq`; on a gap, take a new snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct ApiEvent {
    pub seq: u64,
    /// Milliseconds since the Unix epoch.
    pub ts_ms: u64,
    pub project: String,
    pub body: ApiEventBody,
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
    },
    SessionFailed {
        session: String,
        error: String,
    },
    /// The session was shut down (or its process exited) and its token revoked.
    SessionStopped {
        session: String,
    },
    /// Agent output and lifecycle (stderr lines are not forwarded).
    Agent {
        session: String,
        event: AgentEvent,
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

/// The state of a project as of event `seq`.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub seq: u64,
    pub state: State,
}
