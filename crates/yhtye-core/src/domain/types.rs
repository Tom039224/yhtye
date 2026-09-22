//! Vocabulary shared by the MCP tools and the state machine
//! (`docs/architecture/mcp-tools.md` §2, `orchestration-model.md` §2).

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Role of an agent session. Decides which MCP tools it sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Orchestrator,
    Implementer,
    Reviewer,
}

impl Role {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Implementer => "implementer",
            Self::Reviewer => "reviewer",
        }
    }

    /// Implementer and reviewer sessions are bound to one task step.
    #[must_use]
    pub fn is_sub_agent(self) -> bool {
        !matches!(self, Self::Orchestrator)
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Implement,
    Review,
    Checkpoint,
    Done,
}

impl StepKind {
    /// Role of the session that runs a step of this kind (`None`: run by Yhtye itself).
    #[must_use]
    pub fn role(self) -> Option<Role> {
        match self {
            Self::Implement => Some(Role::Implementer),
            Self::Review => Some(Role::Reviewer),
            Self::Checkpoint | Self::Done => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// Writes code in a dedicated worktree; merged into the group branch when done.
    Code,
    /// Read-only investigation; only the result text is kept.
    Investigate,
}

/// One step of a task as given by the orchestrator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepSpec {
    pub kind: StepKind,
    /// Instruction for this step; the task instruction is used when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
}

impl StepSpec {
    #[must_use]
    pub fn new(kind: StepKind) -> Self {
        Self {
            kind,
            instruction: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approve,
    NeedsChanges,
}

/// Kinds of `help` an agent can raise with the `help` tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentHelpKind {
    Blocked,
    Question,
    Policy,
}

/// Every kind of help: the agent kinds plus the ones Yhtye raises itself
/// (`mcp-tools.md` §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelpKind {
    Blocked,
    Question,
    Policy,
    MergeConflict,
    ReviewRoundsExhausted,
    ProtocolViolation,
    AgentStopped,
    AgentCrashed,
    DirtyReadonlyTree,
    /// A git operation failed for a reason other than a conflict (Stage 3a addition).
    GitFailed,
}

impl HelpKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Question => "question",
            Self::Policy => "policy",
            Self::MergeConflict => "merge_conflict",
            Self::ReviewRoundsExhausted => "review_rounds_exhausted",
            Self::ProtocolViolation => "protocol_violation",
            Self::AgentStopped => "agent_stopped",
            Self::AgentCrashed => "agent_crashed",
            Self::DirtyReadonlyTree => "dirty_readonly_tree",
            Self::GitFailed => "git_failed",
        }
    }
}

impl From<AgentHelpKind> for HelpKind {
    fn from(kind: AgentHelpKind) -> Self {
        match kind {
            AgentHelpKind::Blocked => Self::Blocked,
            AgentHelpKind::Question => Self::Question,
            AgentHelpKind::Policy => Self::Policy,
        }
    }
}

/// Error codes of tool errors (`mcp-tools.md` §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidArgument,
    NotFound,
    InvalidState,
    Forbidden,
    Conflict,
    Internal,
}

/// A tool call that failed in a way the calling agent should read and fix.
/// Sent as `isError: true` with body `{"error": {"code", "message"}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct ToolError {
    pub code: ErrorCode,
    pub message: String,
}

impl ToolError {
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    #[must_use]
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidState, message)
    }

    #[must_use]
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Forbidden, message)
    }

    #[must_use]
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    /// The JSON body sent to the agent.
    #[must_use]
    pub fn body(&self) -> serde_json::Value {
        serde_json::json!({ "error": { "code": self.code, "message": self.message } })
    }
}
