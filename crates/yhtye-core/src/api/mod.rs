//! The UI-facing API (`core-design.md` §2): the event stream ([`ApiEvent`]),
//! the [`Snapshot`], and the commands of [`crate::runtime::Core`]
//! ([`ApiCommand`] → [`ApiResponse`] / [`ApiError`]). The TypeScript types in
//! `src/api/generated/` are generated from these with ts-rs, and the Kotlin
//! types of the Android client from their schemars JSON Schemas (see `typegen`).

mod command;
#[cfg(test)]
mod typegen;
mod wire;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::acp::{AgentEvent, AgentOutput};
use crate::agents::AgentChoice;
use crate::attachments::{AttachmentDelivered, PromptCaps};
use crate::clock::now_ms;
use crate::domain::{DomainEvent, Role, State};
use crate::mcp::ToolCallRecord;
use crate::remote::{Clients, RemoteDevice, RemoteStatus};
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
///
/// App-wide events ([`ApiEventBody::is_app_wide`]: the project list, settings,
/// a resync hint, remote access) have an empty `project`, `seq` 0 and are live.
#[derive(Debug, Clone, Serialize, JsonSchema, TS)]
pub struct ApiEvent {
    pub seq: u64,
    /// Milliseconds since the Unix epoch.
    pub ts_ms: u64,
    pub project: String,
    pub live: bool,
    pub body: ApiEventBody,
}

impl ApiEvent {
    /// An app-wide event (see [`ApiEventBody::is_app_wide`]): no project, `seq`
    /// 0, live, stamped now.
    #[must_use]
    pub fn app_wide(body: ApiEventBody) -> Self {
        Self {
            seq: 0,
            ts_ms: now_ms(),
            project: String::new(),
            live: true,
            body,
        }
    }
}

/// What changed in the settings ([`ApiEventBody::SettingsChanged`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum SettingsKind {
    /// The harness × model settings of a layer (`set_agent_settings`).
    AgentSettings,
    /// The registered harnesses (a manual path was set, or a detection found
    /// other executables).
    Harnesses,
    /// The names of the secret environment variables.
    Secrets,
}

/// Kind of a coalesced text block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    Message,
    Thought,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, JsonSchema, TS)]
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
        /// The ACP session this start meant to restore but could not (it is a
        /// new session instead); kept so the old one can be restored later.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        lost_acp_session_id: Option<String>,
        /// What the agent said it accepts in a prompt (`initialize`): the last
        /// one seen for a harness is what predicts the delivery of attachments.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        prompt_caps: Option<PromptCaps>,
    },
    /// A start of the session failed. `retrying` tells a failure on the way from
    /// the one that gives the start up.
    SessionFailed {
        session: String,
        error: String,
        /// Another start of this session follows (a failed restore is tried
        /// again, or replaced by a new session), so it is still starting. False:
        /// the start is given up and nothing follows. Older logs lack it (the
        /// two were not told apart); read it as false.
        retrying: bool,
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
    /// `text` is the text block; images and text files delivered natively are
    /// further blocks that are not part of it.
    Prompted {
        session: String,
        text: String,
        /// How each attachment of the batch's user messages was delivered, the
        /// final decision (the `inbox_queued` entry has the prediction).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<AttachmentDelivered>,
    },
    /// Live: the session is being started (also a retry of a failed restore).
    /// It ends with `session_started`, `session_stopped` or a `session_failed`
    /// without `retrying`. The sessions starting when a snapshot is taken are
    /// in [`Snapshot::starting`] (T-65: every client sees "starting", not only
    /// the one whose request started it).
    SessionStarting {
        session: String,
    },
    /// App-wide (`project` is empty, `live`, `seq` 0): a project was registered
    /// or opened (its orchestration started in this process), or the project
    /// order changed (a project was used, or the user reordered them; `project`
    /// is the one used, empty for a reorder); clients re-read `list_projects`.
    ProjectsChanged {
        project: String,
    },
    /// App-wide: the order of `project`'s branches changed (a chat of one was
    /// used, or the user reordered them). `keys` is the whole order, as
    /// `get_branch_order` answers it; it replaces what a client has.
    BranchOrderChanged {
        project: String,
        keys: Vec<String>,
    },
    /// App-wide: settings were changed by some client. A hint: a settings
    /// screen on display reads them again (`get_agent_settings`,
    /// `get_harnesses`, `list_secret_env`).
    SettingsChanged {
        kind: SettingsKind,
        /// The project whose agent settings layer changed (`None`: the global
        /// layer, or a kind without projects).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        project: Option<String>,
        /// For `agent_settings`: the new [`AgentSettingsView::revision`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        revision: Option<u64>,
    },
    /// App-wide, sent to one client only: it fell behind and events were
    /// dropped for it (durable ones are in the log, app-wide and live ones are
    /// lost). It catches up: `list_events` from its cursor, `list_projects`,
    /// and the settings and git overview on display.
    Resync,
    /// App-wide (`project` is empty, `live`, `seq` 0): remote access started,
    /// stopped, failed, or its pairing code was issued, used or dropped.
    /// Local only: not sent to remote devices.
    RemoteStatusChanged {
        status: RemoteStatus,
    },
    /// App-wide like `remote_status_changed`: a device was paired, revoked,
    /// connected or disconnected. Local only.
    RemoteDevicesChanged {
        devices: Vec<RemoteDevice>,
    },
    /// App-wide, with every `remote_devices_changed` (a device was paired,
    /// revoked, connected or disconnected): the read-only view of `list_clients`.
    /// Sent to remote devices too.
    ClientsChanged {
        clients: Clients,
    },
}

impl ApiEventBody {
    /// Live events are streamed to clients but not stored (see [`ApiEvent`]).
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.is_app_wide()
            || matches!(
                self,
                Self::SessionStarting { .. }
                    | Self::Agent {
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

    /// Events of the whole app, not of a project (sent with
    /// [`ApiEvent::app_wide`]).
    #[must_use]
    pub fn is_app_wide(&self) -> bool {
        self.is_local_only()
            || matches!(
                self,
                Self::ProjectsChanged { .. }
                    | Self::BranchOrderChanged { .. }
                    | Self::SettingsChanged { .. }
                    | Self::Resync
                    | Self::ClientsChanged { .. }
            )
    }

    /// App-wide events about remote access, for the PC's own UI only (the
    /// counterpart of [`ApiCommand::is_local_only`]).
    #[must_use]
    pub fn is_local_only(&self) -> bool {
        matches!(
            self,
            Self::RemoteStatusChanged { .. } | Self::RemoteDevicesChanged { .. }
        )
    }

    /// The session key the event belongs to, if any.
    #[must_use]
    pub fn session(&self) -> Option<&str> {
        match self {
            Self::Domain { .. }
            | Self::ProjectsChanged { .. }
            | Self::BranchOrderChanged { .. }
            | Self::SettingsChanged { .. }
            | Self::Resync
            | Self::RemoteStatusChanged { .. }
            | Self::RemoteDevicesChanged { .. }
            | Self::ClientsChanged { .. } => None,
            Self::SessionStarted { session, .. }
            | Self::SessionFailed { session, .. }
            | Self::SessionStopped { session, .. }
            | Self::SessionInterrupted { session }
            | Self::SessionStarting { session }
            | Self::Agent { session, .. }
            | Self::AgentText { session, .. }
            | Self::Prompted { session, .. } => Some(session),
            Self::ToolCalled { record } => Some(&record.binding.session),
        }
    }
}

/// The state of a project as of event `seq`: the domain state, the agent
/// sessions (`agent_sessions`, derived from the same events) and the chats.
#[derive(Debug, Clone, Serialize, JsonSchema, TS)]
pub struct Snapshot {
    pub seq: u64,
    pub state: State,
    pub sessions: Vec<SessionRecord>,
    /// The project's chats with their times, most recently used first (Stage 8).
    pub chats: Vec<ChatInfo>,
    /// Sessions being started (T-65): what `session_starting` said and no
    /// `session_started` / `session_stopped` / final `session_failed` ended yet.
    pub starting: Vec<String>,
}
