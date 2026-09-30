//! Commands of [`crate::runtime::Core`] and their results (`core-design.md` §2).
//!
//! One command type for both transports: the Tauri command `yhtye_command(cmd)`
//! and the WebSocket bridge (`{id, cmd}`, see [`super::WsRequest`]) carry the
//! same [`ApiCommand`] JSON and return the same [`ApiResponse`] / [`ApiError`].

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::{ApiEventBody, Snapshot};
use crate::agents::{
    AgentChoice, AgentRole, AgentSettings, AgentSettingsLayer, HarnessInfo, HarnessModels,
    ModelEfforts, RoleSettings,
};
use crate::domain::{ErrorCode, ToolError};
use crate::git::GitOverview;
use crate::secrets::SecretValue;
use crate::store::{ChatInfo, StoreError, StoredEvent};
use crate::usage::UsageReport;

/// Default page size of [`ApiCommand::ListEvents`].
pub const DEFAULT_EVENT_PAGE: u32 = 500;
/// Largest page size of [`ApiCommand::ListEvents`].
pub const MAX_EVENT_PAGE: u32 = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApiCommand {
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
    SendUserMessage {
        project: String,
        chat: String,
        text: String,
    },
    /// Cancels the running turn of the chat's orchestrator, if any.
    CancelOrchestratorTurn {
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
    SetAgentSettings {
        #[serde(default)]
        #[ts(optional)]
        project: Option<String>,
        role: AgentRole,
        settings: Option<RoleSettings>,
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
    /// The names of the secret environment variables (Stage 7e). Values are
    /// never returned.
    ListSecretEnv,
    /// Stores `value` in the OS credential store under `name` (replacing an
    /// earlier one) and registers the name; every agent started afterwards gets
    /// the variable. `unavailable` when the credential store cannot be used.
    SetSecretEnv {
        name: String,
        #[ts(type = "string")]
        value: SecretValue,
    },
    /// Removes the value and the name.
    DeleteSecretEnv {
        name: String,
    },
}

/// Everything the settings panel shows (`core-design.md` §15.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
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
}

/// Default number of commits of [`ApiCommand::GetGitOverview`].
pub const DEFAULT_GRAPH_COMMITS: u32 = 120;

/// A project known to Yhtye.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct ProjectInfo {
    pub id: String,
    /// Display name (the directory name).
    pub name: String,
    pub path: String,
    /// Its orchestration is running in this process.
    pub open: bool,
}

/// A stored (durable) event, in the same shape as [`super::ApiEvent`].
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct LoggedEvent {
    pub seq: u64,
    pub ts_ms: u64,
    pub project: String,
    /// Always `false` (only durable events are stored).
    pub live: bool,
    #[ts(as = "ApiEventBody")]
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
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiResponse {
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
    Chats {
        chats: Vec<ChatInfo>,
    },
    Chat {
        chat: ChatInfo,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, thiserror::Error)]
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
