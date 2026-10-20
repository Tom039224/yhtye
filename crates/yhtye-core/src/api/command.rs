//! Commands of [`crate::runtime::Core`] and their results (`core-design.md` §2).
//!
//! One command type for both transports: the Tauri command `yhtye_command(cmd)`
//! and the WebSocket bridge (`{id, cmd}`, see [`super::WsRequest`]) carry the
//! same [`ApiCommand`] JSON and return the same [`ApiResponse`] / [`ApiError`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::{ApiEventBody, Snapshot};
use crate::agents::{
    AgentChoice, AgentRole, AgentSettings, AgentSettingsLayer, HarnessDetection, HarnessInfo,
    HarnessModels, ModelEfforts, RoleSettings,
};
use crate::attachments::AttachmentMeta;
use crate::domain::{ErrorCode, ToolError};
use crate::git::GitOverview;
use crate::remote::{Clients, PairingCode, RemoteDevice, RemoteStatus};
use crate::secrets::SecretValue;
use crate::store::{ChatInfo, StoreError, StoredEvent};
use crate::usage::UsageReport;

/// Default page size of [`ApiCommand::ListEvents`].
pub const DEFAULT_EVENT_PAGE: u32 = 500;
/// Largest page size of [`ApiCommand::ListEvents`].
pub const MAX_EVENT_PAGE: u32 = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApiCommand {
    /// A liveness check of the core: answers with the name of the machine it
    /// runs on (the host footer of the sidebar). The UI sends it periodically.
    Ping,
    /// Every project known to the database, and whether it is open.
    ListProjects,
    /// Opens (registering it on first use) the git repository at `path`: starts
    /// its orchestrator and resumes interrupted tasks. Opening an open project
    /// returns it unchanged.
    OpenProject {
        path: String,
    },
    GetSnapshot {
        project: String,
    },
    /// Durable events with `seq > after_seq`, oldest first (history and
    /// catch-up after a gap). Works for projects that are not open.
    ListEvents {
        project: String,
        after_seq: u64,
        #[serde(default)]
        #[ts(optional)]
        limit: Option<u32>,
    },
    /// Queues a `user_message` for the orchestrator of `chat` (sent when it is
    /// idle). Starts that orchestrator if it is not running (Stage 8: lazily,
    /// restoring the chat's stored session if there is one). `not_found` for an
    /// unknown chat, `invalid_state` when the chat's worktree no longer exists.
    /// `attachments` are files staged with `stage_attachment` for this chat; the
    /// text may be empty when there is at least one (`invalid_argument` if there
    /// is neither). `not_found` for an attachment that does not exist,
    /// `invalid_argument` for one that was sent before.
    SendUserMessage {
        project: String,
        chat: String,
        text: String,
        /// Ids from `attachment_staged`, in the order to show them (a client
        /// without attachments leaves it out).
        #[serde(default)]
        #[ts(optional = nullable)]
        attachments: Vec<String>,
    },
    /// Saves a file the user wants to attach to a message of `chat` (the project
    /// must be registered, the chat must exist). `data_base64` is the whole file,
    /// of any size (a transport may limit it). `name` is made safe to save under
    /// (directory parts are dropped); `mime` is the client's claim, corrected by
    /// the content. Answers `attachment_staged`: its id for `send_user_message`,
    /// its kind, and how it is expected to be delivered to the chat's
    /// orchestrator (`delivery`): `native` when the harness it runs accepts that
    /// kind (images, or text as embedded context), else `path`. The final choice
    /// is made when the message is sent, with the orchestrator actually running;
    /// the `prompted` event says what it was. `invalid_argument` for bad base64.
    StageAttachment {
        project: String,
        chat: String,
        name: String,
        #[serde(default)]
        #[ts(optional)]
        mime: Option<String>,
        data_base64: String,
    },
    /// Deletes a staged attachment that was not sent. `not_found` if there is
    /// none, `invalid_state` once a message was sent with it.
    DiscardAttachment {
        project: String,
        chat: String,
        id: String,
    },
    /// A staged or sent attachment with its content (previews in the
    /// conversation, also on remote devices). `not_found` if it does not exist
    /// (or its chat was deleted).
    GetAttachment {
        project: String,
        chat: String,
        id: String,
    },
    /// The user opened `chat` in the UI: its orchestrator is started now,
    /// restoring its stored session, if it is not running and has anything to
    /// resume (it ran when Yhtye stopped, the chat has an open group, or it has
    /// inbox entries). An app restart starts no orchestrator; this is where a
    /// chat comes back (also when something is sent to it). Answers
    /// `chat_opened`; the start itself goes on in the background and shows as
    /// `session_started` (or `session_failed`). `not_found` for an unknown chat.
    OpenChat {
        project: String,
        chat: String,
    },
    /// Cancels the running turn of the chat's orchestrator, if any.
    CancelOrchestratorTurn {
        project: String,
        chat: String,
    },
    /// Sends [`crate::domain::COMPACT_PROMPT`] (`/compact`) as is, not through
    /// the inbox, to the chat's orchestrator session: the harness compacts its
    /// context (Claude Code does; others may answer it as a plain message).
    /// Inbox entries that arrive meanwhile are delivered after that turn.
    /// `not_found` for an unknown chat, `invalid_state` unless the orchestrator
    /// is running and idle (not started, starting, or in a turn).
    CompactChat {
        project: String,
        chat: String,
    },
    /// The project's chats, most recently used first. Works for projects that
    /// are not open.
    ListChats {
        project: String,
    },
    /// A new chat (its orchestrator is not started), bound to a worktree
    /// (Stage 8e): give exactly one of `branch` (its worktree: where it is
    /// checked out, else a new Yhtye one) or `worktree` (an existing worktree,
    /// as `get_git_overview` lists it). `not_found` for a missing branch or an
    /// unknown worktree, `invalid_argument` for `yhtye/*`, for both or neither.
    CreateChat {
        project: String,
        #[serde(default)]
        #[ts(optional)]
        branch: Option<String>,
        #[serde(default)]
        #[ts(optional)]
        worktree: Option<String>,
    },
    /// Gives a chat the user's own title (whitespace is flattened; at most
    /// [`crate::domain::MAX_RENAMED_TITLE_CHARS`] characters), which the first
    /// message no longer replaces. `invalid_argument` for an empty or too long
    /// title, `not_found` for an unknown chat. The project must be open.
    RenameChat {
        project: String,
        chat: String,
        title: String,
    },
    /// Deletes a chat: its stored conversation state, its finished groups with
    /// their tasks, its inbox and its orchestrator's stored session. Its
    /// worktree and branch are not touched, and the event log keeps the history
    /// (the log is append-only). `not_found` for an unknown chat,
    /// `invalid_state` while its orchestrator is working or it has a group that
    /// is not finished (`active`, `finishing`, `merge_blocked`); an idle
    /// orchestrator process is stopped. The project must be open.
    DeleteChat {
        project: String,
        chat: String,
    },
    /// Creates a branch in a Yhtye worktree (the main clone keeps its checkout)
    /// and its first chat. `from` defaults to the main worktree's HEAD.
    /// `invalid_argument` for a bad or reserved (`yhtye/*`) name, `conflict` if
    /// the branch exists, `not_found` if `from` does not.
    CreateBranch {
        project: String,
        name: String,
        #[serde(default)]
        #[ts(optional)]
        from: Option<String>,
    },
    /// Cancels a task as the user (like the orchestrator's `cancel_task`); the
    /// orchestrator is told with a `user_message`.
    CancelTask {
        project: String,
        task: String,
        #[serde(default)]
        #[ts(optional)]
        reason: Option<String>,
    },
    /// Cancels a group as the user (like `cancel_group`); the orchestrator is told.
    CancelGroup {
        project: String,
        group: String,
        #[serde(default)]
        #[ts(optional)]
        reason: Option<String>,
    },
    /// Retries the base merge of a `merge_blocked` group. Accepted means the
    /// merge ran; whether it succeeded is in the group's status (and the
    /// orchestrator gets a `merge_result`).
    RetryGroupMerge {
        project: String,
        group: String,
    },
    /// The project repository's checked-out branch, local branches and recent
    /// commit graph (the git panel, Stage 6a). Works for projects that are not open.
    GetGitOverview {
        project: String,
        /// Most commits to return (default [`DEFAULT_GRAPH_COMMITS`], at most 500).
        #[serde(default)]
        #[ts(optional)]
        limit: Option<u32>,
    },
    /// Subscription usage / quota of the harness (5-hour and weekly windows,
    /// plan name), Stage 6b. Cached for a minute; `refresh` asks again unless
    /// the cached report is only seconds old. `unavailable` when the harness
    /// does not report it.
    GetUsage {
        #[serde(default)]
        #[ts(optional)]
        refresh: Option<bool>,
    },
    /// The harness × model settings of every role (Stage 7b): registered
    /// harnesses, the global layer and, with `project`, its layer and the
    /// settings in effect there (the project must be registered, not open).
    GetAgentSettings {
        #[serde(default)]
        #[ts(optional)]
        project: Option<String>,
    },
    /// Replaces one role of the global layer (no `project`) or a project's
    /// layer; `settings: null` removes it (inherit). Applies to sessions
    /// started afterwards. Returns the settings like `get_agent_settings`.
    /// With `expected_revision` (the `revision` of the settings the screen
    /// shows), `conflict` if they were changed since (by another client), so an
    /// old screen does not overwrite them.
    SetAgentSettings {
        #[serde(default)]
        #[ts(optional)]
        project: Option<String>,
        role: AgentRole,
        settings: Option<RoleSettings>,
        #[serde(default)]
        #[ts(optional)]
        expected_revision: Option<u64>,
    },
    /// The models a harness offers, read from it over ACP (a short session
    /// without a prompt). Cached for 10 minutes; `refresh` asks again unless
    /// the cached list is only seconds old.
    ListHarnessModels {
        harness: String,
        #[serde(default)]
        #[ts(optional)]
        refresh: Option<bool>,
    },
    /// The efforts (thought levels) one model of a harness supports, for
    /// models whose efforts `list_harness_models` did not include (harnesses
    /// with many models). Cached for 10 minutes; costs one short listing
    /// session the first time.
    ListModelEfforts {
        harness: String,
        model: String,
    },
    /// Which harnesses are installed: looks for their executables again (the
    /// `PATH`, a few well-known directories, and the paths set with
    /// `set_harness_path`) and registers those that were found, so the settings
    /// panel offers them. Sessions already running are not affected. Called when
    /// the settings are opened.
    GetHarnesses,
    /// Like `get_harnesses`, and also forgets the models read from every
    /// harness (the "detect again" button).
    DetectHarnesses,
    /// Sets (or with `path: null` removes: detect automatically) the path of a
    /// harness's main executable (`npx` for Claude Code), used before the `PATH`
    /// search. `invalid_argument` unless it is an absolute path of an executable
    /// file, `not_found` for an unknown harness. Detects again and returns the
    /// harnesses like `get_harnesses`.
    SetHarnessPath {
        harness: String,
        path: Option<String>,
    },
    /// The names of the secret environment variables (Stage 7e). Values are
    /// never returned.
    ListSecretEnv,
    /// Stores `value` in the OS credential store under `name` (replacing an
    /// earlier one) and registers the name; every agent started afterwards gets
    /// the variable. `unavailable` when the credential store cannot be used.
    SetSecretEnv {
        name: String,
        #[ts(type = "string")]
        #[schemars(with = "String")]
        value: SecretValue,
    },
    /// Removes the value and the name.
    DeleteSecretEnv {
        name: String,
    },
    /// Remote access (`core-design.md` §18): its settings, whether it is
    /// listening and where, and why not. Local only (like every `*remote*` /
    /// pairing command; see [`ApiCommand::is_local_only`]).
    GetRemoteStatus,
    /// Turns remote access on or off and sets its port (`port` absent: keep
    /// it; default 1423). Stored, and applied at once: starts, restarts or stops
    /// the server. Turning it on when it failed to start tries again. A failure
    /// to start (Tailscale missing or stopped, the port taken) is not an error
    /// of the command: it is the status's `error`. `invalid_argument` for port 0.
    SetRemoteSettings {
        enabled: bool,
        #[serde(default)]
        #[ts(optional)]
        port: Option<u16>,
    },
    /// Issues a pairing code (valid for 5 minutes, once; the previous code stops
    /// working) and the QR code's URI. `invalid_state` unless the server is running.
    CreatePairingCode,
    /// The paired devices, oldest first.
    ListRemoteDevices,
    /// Forgets a paired device and closes its connections at once; its token
    /// stops working. `not_found` for an unknown device.
    RevokeRemoteDevice {
        device: String,
    },
    /// This PC (its host name) and the paired devices with whether each is
    /// connected (`core-design.md` §18.8): a read-only view for every client,
    /// remote devices too (not local only). `clients_changed` says when it changed.
    ListClients,
}

impl ApiCommand {
    /// Commands that only the PC itself may run: a paired remote device may
    /// not change remote access, issue pairing codes or manage devices (it
    /// could otherwise add devices or keep itself from being revoked).
    #[must_use]
    pub fn is_local_only(&self) -> bool {
        matches!(
            self,
            Self::GetRemoteStatus
                | Self::SetRemoteSettings { .. }
                | Self::CreatePairingCode
                | Self::ListRemoteDevices
                | Self::RevokeRemoteDevice { .. }
        )
    }
}

/// Everything the settings panel shows (`core-design.md` §15.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct AgentSettingsView {
    pub harnesses: Vec<HarnessInfo>,
    /// Used for a role no layer sets.
    pub builtin: AgentChoice,
    pub global: AgentSettingsLayer,
    /// The project asked for, with its layer.
    pub project: Option<String>,
    pub project_layer: Option<AgentSettingsLayer>,
    /// In effect for `project` (or globally).
    pub effective: AgentSettings,
    /// Version of all agent settings layers: it grows with every change (also
    /// across restarts of the core), so a larger one is newer. Passed back as
    /// `set_agent_settings`' `expected_revision`.
    pub revision: u64,
}

/// Default number of commits of [`ApiCommand::GetGitOverview`].
pub const DEFAULT_GRAPH_COMMITS: u32 = 120;

/// A project known to Yhtye.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct ProjectInfo {
    pub id: String,
    /// Display name (the directory name).
    pub name: String,
    pub path: String,
    /// Its orchestration is running in this process.
    pub open: bool,
    /// Work is on in it: a live agent session is in a turn, or a task is
    /// running or merging (what the sidebar's turning ring shows). Always
    /// `false` while it is not open.
    pub running: bool,
}

/// A stored (durable) event, in the same shape as [`super::ApiEvent`].
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, TS)]
pub struct LoggedEvent {
    pub seq: u64,
    pub ts_ms: u64,
    pub project: String,
    /// Always `false` (only durable events are stored).
    pub live: bool,
    #[ts(as = "ApiEventBody")]
    #[schemars(with = "ApiEventBody")]
    pub body: Value,
}

impl LoggedEvent {
    #[must_use]
    pub fn from_stored(project: &str, e: StoredEvent) -> Self {
        Self {
            seq: e.seq,
            ts_ms: e.ts_ms,
            project: project.to_string(),
            live: false,
            body: e.body,
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiResponse {
    /// The core is alive; `host` is the name of the machine it runs on and
    /// `version` the version of the Yhtye desktop app (`0.1.0`).
    Pong {
        host: String,
        /// A core that predates the field leaves it out, which a reader takes
        /// as "unknown" (the Android client checks it against its own version).
        #[serde(default)]
        version: String,
    },
    Projects {
        projects: Vec<ProjectInfo>,
    },
    Project {
        project: ProjectInfo,
    },
    Snapshot {
        snapshot: Snapshot,
    },
    Events {
        events: Vec<LoggedEvent>,
        /// More events follow (the page was full).
        more: bool,
    },
    /// The command was accepted (sending, cancelling).
    Accepted,
    /// Answer of `stage_attachment`.
    AttachmentStaged {
        attachment: AttachmentMeta,
    },
    /// Answer of `get_attachment`.
    Attachment {
        attachment: AttachmentMeta,
        data_base64: String,
    },
    Chats {
        chats: Vec<ChatInfo>,
    },
    Chat {
        chat: ChatInfo,
    },
    /// Answer of `open_chat`.
    ChatOpened {
        /// The chat's orchestrator is being started (now, or by an earlier call).
        starting: bool,
    },
    GitOverview {
        git: GitOverview,
    },
    Usage {
        usage: UsageReport,
    },
    AgentSettings {
        settings: Box<AgentSettingsView>,
    },
    HarnessModels {
        models: HarnessModels,
    },
    ModelEfforts {
        efforts: ModelEfforts,
    },
    /// The registered secret environment variable names, sorted.
    SecretEnv {
        names: Vec<String>,
    },
    /// Every known harness with whether it was found and where. Empty when the
    /// core does not detect harnesses (tests).
    Harnesses {
        harnesses: Vec<HarnessDetection>,
    },
    /// Answer of `get_remote_status` and `set_remote_settings`.
    RemoteStatus {
        status: RemoteStatus,
    },
    /// Answer of `create_pairing_code`.
    PairingCode {
        pairing: PairingCode,
    },
    /// Answer of `list_remote_devices` and `revoke_remote_device`.
    RemoteDevices {
        devices: Vec<RemoteDevice>,
    },
    /// Answer of `list_clients`.
    Clients {
        clients: Clients,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    InvalidArgument,
    NotFound,
    InvalidState,
    Conflict,
    Forbidden,
    /// The project's orchestration (or the core) has stopped.
    Unavailable,
    Internal,
}

/// A failed command. `message` is meant to be shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct ApiError {
    pub code: ApiErrorCode,
    pub message: String,
}

impl ApiError {
    #[must_use]
    pub fn new(code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::InvalidArgument, message)
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::NotFound, message)
    }

    #[must_use]
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::Conflict, message)
    }

    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::Unavailable, message)
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ApiErrorCode::Internal, message)
    }
}

impl From<ToolError> for ApiError {
    fn from(e: ToolError) -> Self {
        let code = match e.code {
            ErrorCode::InvalidArgument => ApiErrorCode::InvalidArgument,
            ErrorCode::NotFound => ApiErrorCode::NotFound,
            ErrorCode::InvalidState => ApiErrorCode::InvalidState,
            ErrorCode::Forbidden => ApiErrorCode::Forbidden,
            ErrorCode::Conflict => ApiErrorCode::Conflict,
            ErrorCode::Internal => ApiErrorCode::Internal,
        };
        Self::new(code, e.message)
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        Self::internal(e.to_string())
    }
}
