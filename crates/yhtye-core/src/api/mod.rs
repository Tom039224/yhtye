//! The UI-facing API (`core-design.md` §2): the event stream ([`ApiEvent`]),
//! the [`Snapshot`], and the commands of [`crate::runtime::Core`]
//! ([`ApiCommand`] → [`ApiResponse`] / [`ApiError`]). The TypeScript types in
//! `src/api/generated/` are generated from these with ts-rs (see `typegen`).

mod command;
#[cfg(test)]
mod typegen;
mod wire;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::acp::{AgentEvent, AgentOutput};
use crate::agents::AgentChoice;
use crate::domain::{DomainEvent, Role, State};
use crate::mcp::ToolCallRecord;
use crate::store::{ChatInfo, SessionRecord};

pub use command::{
    AgentSettingsView, ApiCommand, ApiError, ApiErrorCode, ApiResponse, DEFAULT_EVENT_PAGE,
    DEFAULT_GRAPH_COMMITS, LoggedEvent, MAX_EVENT_PAGE, ProjectInfo,
};
pub use wire::{WsReply, WsRequest, WsServerMessage};

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
/// `seq > snapshot.seq`; on a gap between durable events, fetch the missing ones
/// with [`ApiCommand::ListEvents`] (or take a new snapshot). Live events are for
/// display only and never folded into state.
#[derive(Debug, Clone, Serialize, TS)]
pub struct ApiEvent {
    pub seq: u64,
    /// Milliseconds since the Unix epoch.
    pub ts_ms: u64,
    pub project: String,
    pub live: bool,
    pub body: ApiEventBody,
}

/// Kind of a coalesced text block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    Message,
    Thought,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, TS)]
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
        /// The harness × model it runs (Stage 7b; absent in older logs).
        agent: Option<AgentChoice>,
        /// What it was meant to run when that harness is not registered (any
        /// more) and `agent` runs instead (Stage 7c-2; shown to the user).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        replaced: Option<AgentChoice>,
        /// The directory it was started in (Stage 8; absent in older logs).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        cwd: Option<String>,
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

/// The state of a project as of event `seq`: the domain state, the agent
/// sessions (`agent_sessions`, derived from the same events) and the chats.
#[derive(Debug, Clone, Serialize, TS)]
pub struct Snapshot {
    pub seq: u64,
    pub state: State,
    pub sessions: Vec<SessionRecord>,
    /// The project's chats with their times, most recently used first (Stage 8).
    pub chats: Vec<ChatInfo>,
}
