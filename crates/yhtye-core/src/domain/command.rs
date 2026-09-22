//! Inputs of the state machine ([`DomainCommand`]) and what it asks the runtime
//! to do ([`Effect`]) (`core-design.md` §5).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::event::DomainEvent;
use super::types::{Role, TaskKind, ToolError};
use crate::mcp::SessionBinding;
use crate::mcp::tools::{CreateGroupArgs, ToolCall};

/// The agent session working on one step of a task. The runtime keeps one
/// implementer session per task and a fresh reviewer session per review step.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentRef {
    pub task: String,
    pub role: Role,
    pub step: usize,
}

/// How an agent's turn ended (from ACP `session/prompt`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "detail", rename_all = "snake_case")]
pub enum TurnOutcome {
    EndTurn,
    Cancelled,
    /// `max_tokens` / `max_turn_requests` / `refusal`.
    Stopped(String),
    /// The connection closed mid-turn (the process exit follows separately).
    Closed,
    /// The prompt request failed.
    Error(String),
}

/// A git operation the runtime runs through [`crate::git::GitService`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GitOp {
    CreateGroupBranch {
        group: String,
        group_branch: String,
        base_branch: String,
    },
    /// Worktree of a task: its own for `code`, the group's for `investigate`.
    PrepareWorkspace {
        group: String,
        task: String,
        kind: TaskKind,
    },
    /// `code`: commit leftovers, merge into the group branch, remove the worktree.
    /// `investigate`: check that the tree is still clean.
    FinishTask {
        group: String,
        task: String,
        kind: TaskKind,
        message: String,
    },
    RemoveWorkspace {
        group: String,
        task: String,
    },
    MergeGroup {
        group: String,
        group_branch: String,
        base_branch: String,
    },
}

/// Result of a [`GitOp`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum GitResult {
    Done,
    Workspace {
        path: PathBuf,
    },
    Merged {
        detail: String,
    },
    Conflict {
        files: Vec<String>,
    },
    /// An `investigate` task left changes behind.
    Dirty {
        files: Vec<String>,
    },
    /// `MergeGroup` could not run (base tree dirty or on another branch).
    Blocked {
        detail: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainCommand {
    /// `create_group`, with the base branch the runtime read from the main
    /// worktree (`None`: detached HEAD or no branch).
    CreateGroup {
        args: CreateGroupArgs,
        base_branch: Option<String>,
    },
    /// Any other tool call.
    Tool {
        binding: SessionBinding,
        call: ToolCall,
    },
    UserMessage {
        text: String,
    },
    TurnEnded {
        agent: AgentRef,
        outcome: TurnOutcome,
        /// Another prompt (e.g. a help answer that arrived mid-turn) is already
        /// queued for the session, so the step continues in the next turn.
        prompt_queued: bool,
    },
    /// The agent's process exited without Yhtye stopping it.
    AgentExited {
        agent: AgentRef,
        detail: String,
    },
    AgentStartFailed {
        agent: AgentRef,
        error: String,
    },
    GitDone {
        op: GitOp,
        result: GitResult,
    },
    /// The runtime sent every inbox entry up to `up_to` to the orchestrator.
    InboxDelivered {
        up_to: u64,
    },
    /// Yhtye started again on a stored state (`orchestration-model.md` §10):
    /// tasks that were mid-turn or mid-merge become `interrupted`, agents that were
    /// waiting for a help answer are marked lost, an interrupted group merge is
    /// reported as blocked, and the orchestrator is told what it missed.
    Restart {
        orchestrator: OrchestratorResume,
    },
    /// Continues an `interrupted` task (sent by the runtime after `Restart`).
    ResumeTask {
        task: String,
    },
}

/// What happened to the orchestrator's session on restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestratorResume {
    /// There was a session before the restart.
    pub had_session: bool,
    /// That session was restored with `session/load`.
    pub restored: bool,
    /// Its turn was still running when Yhtye stopped.
    pub turn_was_running: bool,
}

/// Side effects for the runtime, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    /// Send the step prompt to the step's agent session (starting it if needed).
    RunStep {
        agent: AgentRef,
        group: String,
        prompt: String,
        workdir: Option<PathBuf>,
    },
    /// Continue an interrupted step after a restart: restore the step's session
    /// with `session/load` and send `prompt`; if it cannot be restored, start a new
    /// session and send `fallback` (the full step prompt with a restart note).
    ResumeStep {
        agent: AgentRef,
        group: String,
        prompt: String,
        fallback: String,
        workdir: Option<PathBuf>,
    },
    /// Send a follow-up prompt (help answer, reminder) to the step's session.
    PromptAgent {
        agent: AgentRef,
        text: String,
    },
    /// Stop one agent session (a reviewer whose review is done).
    StopAgent {
        agent: AgentRef,
    },
    /// Stop every session of the task (it reached a terminal state).
    StopTaskAgents {
        task: String,
    },
    Git(GitOp),
    /// The inbox has new entries; deliver them when the orchestrator is idle.
    WakeOrchestrator,
}

/// The result of one command.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Transition {
    pub events: Vec<DomainEvent>,
    pub effects: Vec<Effect>,
    /// The tool reply, when the command answers a tool call. A follow-up command
    /// (e.g. a git result) may produce the final reply of the original call.
    pub reply: Option<Result<Value, ToolError>>,
}
