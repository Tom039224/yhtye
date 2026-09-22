//! Commands of [`crate::runtime::Core`] and their results (`core-design.md` §2).
//!
//! One command type for both transports: the Tauri command `yhtye_command(cmd)`
//! and the WebSocket bridge (`{id, cmd}`, see [`super::WsRequest`]) carry the
//! same [`ApiCommand`] JSON and return the same [`ApiResponse`] / [`ApiError`].

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::{ApiEventBody, Snapshot};
use crate::domain::{ErrorCode, ToolError};
use crate::git::GitOverview;
use crate::store::{StoreError, StoredEvent};

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
    /// Queues a `user_message` for the orchestrator (sent when it is idle).
    SendUserMessage {
        project: String,
        text: String,
    },
    /// Cancels the orchestrator's running turn, if any.
    CancelOrchestratorTurn {
        project: String,
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
    GitOverview {
        git: GitOverview,
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
