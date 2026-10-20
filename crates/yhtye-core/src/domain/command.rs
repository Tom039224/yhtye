//! Inputs of the state machine ([`DomainCommand`]) and what it asks the runtime
//! to do ([`Effect`]) (`core-design.md` §5).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::event::DomainEvent;
use super::types::{MessageOutcome, MessageVia, Role, TaskKind, ToolError};
use crate::agents::AgentChoice;
use crate::attachments::AttachmentMeta;
use crate::mcp::SessionBinding;
use crate::mcp::tools::{CreateGroupArgs, CreateTaskArgs, MessageTaskArgs, ToolCall};

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
    /// Recreates the group branch / integration worktree if they are missing.
    PrepareWorkspace {
        group: String,
        task: String,
        kind: TaskKind,
        group_branch: String,
        base_branch: String,
    },
    /// `code`: commit leftovers, squash into the group branch as one commit,
    /// remove the worktree. `investigate`: check that the tree is still clean.
    FinishTask {
        group: String,
        task: String,
        kind: TaskKind,
        /// Message of the commit of what the agents left uncommitted.
        message: String,
        group_branch: String,
        base_branch: String,
        /// The task's title: the squash commit's subject (empty in older ops).
        #[serde(default)]
        title: String,
        /// The agent that implemented the task, as its session was started
        /// (the runtime fills it in; `None`: unknown).
        #[serde(default)]
        agent: Option<AgentChoice>,
        /// The agents of the task's review steps, oldest first, without repeats.
        #[serde(default)]
        reviewers: Vec<AgentChoice>,
    },
    /// A cancelled `code` task: commit leftovers to its branch, remove the worktree.
    RemoveWorkspace { group: String, task: String },
    /// A cancelled group: remove its integration worktree (the branch is kept).
    RemoveGroupWorkspace { group: String },
    /// Merges the group branch into `base_branch` in `worktree` (the group's
    /// chat's), if the worktree has `base_branch` checked out and is clean
    /// (Stage 8e: anything else is `Blocked`, nothing is merged).
    MergeGroup {
        group: String,
        worktree: PathBuf,
        group_branch: String,
        base_branch: String,
        /// Who started the merge; decides whether the result also goes to the
        /// orchestrator's inbox as `merge_result`.
        #[serde(default)]
        trigger: MergeTrigger,
        /// The group's title and `finish_group`'s summary (empty: none), for
        /// the merge commit's message.
        #[serde(default)]
        title: String,
        #[serde(default)]
        summary: String,
        /// The `code` tasks that were merged into the group (done ones).
        #[serde(default)]
        tasks: Vec<GroupTask>,
    },
}

/// A task of a group as the group's merge commit lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupTask {
    pub id: String,
    pub title: String,
}

/// Who started a group's merge into its base branch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeTrigger {
    /// `finish_group`: the result is the tool's reply.
    #[default]
    FinishGroup,
    /// The user retried a blocked merge (`RetryGroupMerge`): reported as `merge_result`.
    UserRetry,
    /// Yhtye finished a settled group the orchestrator left open after a
    /// reminder (Stage 7a): reported as `merge_result`.
    Yhtye,
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

#[allow(clippy::large_enum_variant)] // `create_task` carries the agent arguments
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainCommand {
    /// A new chat in `worktree` (the runtime resolved it and checked that it
    /// is an existing worktree of the repository).
    CreateChat {
        worktree: PathBuf,
    },
    /// The user renames `chat` (`invalid_argument` for an empty or too long
    /// title, `not_found` for an unknown chat).
    RenameChat {
        chat: String,
        title: String,
    },
    /// The user deletes `chat` with its finished groups. Refused
    /// (`invalid_state`) while it has a group that is not finished.
    DeleteChat {
        chat: String,
    },
    /// `create_group` by the orchestrator of `chat`, merging into `base_branch`:
    /// the branch the chat's worktree has checked out now (read by the runtime).
    CreateGroup {
        chat: String,
        args: CreateGroupArgs,
        base_branch: String,
        /// What in git already uses the group's name (a branch or worktree
        /// directory left by an earlier database, read by the runtime); `None`:
        /// nothing. The name is then refused, like one the state has.
        taken_in_git: Option<String>,
    },
    /// `create_task` (a tool call of `binding`), with what in git already uses
    /// the task's name (its branch or worktree directory; see `CreateGroup`).
    CreateTask {
        binding: SessionBinding,
        args: CreateTaskArgs,
        taken_in_git: Option<String>,
    },
    /// Any other tool call.
    Tool {
        binding: SessionBinding,
        call: ToolCall,
    },
    /// `message_task` by the orchestrator. Answered later, when the runtime has
    /// delivered the message or handed it back ([`DomainCommand::MessageResolved`]);
    /// only a refusal, or a message the state already rules out, is answered here.
    MessageTask {
        binding: SessionBinding,
        args: MessageTaskArgs,
        /// An earlier message to the same task is still being delivered (the
        /// runtime knows; the state does not).
        pending: bool,
    },
    /// The runtime settled a message that [`Effect::DeliverMessage`] asked it to
    /// deliver: the answer of the `message_task` call.
    MessageResolved {
        task: String,
        step: usize,
        outcome: MessageOutcome,
        via: Option<MessageVia>,
        detail: String,
    },
    UserMessage {
        chat: String,
        text: String,
        /// Staged attachments sent with it.
        #[serde(default)]
        attachments: Vec<AttachmentMeta>,
    },
    /// The orchestrator's turn ended (Stage 7a: a settled group it left open
    /// gets a reminder, then Yhtye finishes it).
    OrchestratorTurnEnded {
        chat: String,
        outcome: TurnOutcome,
        /// A prompt is already queued for the orchestrator.
        prompt_queued: bool,
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
    /// The runtime sent every inbox entry of `chat` up to `up_to` to its orchestrator.
    InboxDelivered {
        chat: String,
        up_to: u64,
    },
    /// Yhtye started again on a stored state (`orchestration-model.md` §10):
    /// tasks that were mid-turn or mid-merge become `interrupted`, agents that were
    /// waiting for a help answer are marked lost, and an interrupted group merge is
    /// reported as blocked. No orchestrator is started with it: each chat's is
    /// started when it is opened or has something to receive, and is told what it
    /// missed then (`TellOrchestrator`).
    Restart,
    /// The orchestrator of `chat` is started again (after a restart, or lazily
    /// after its process ended): queues what it missed, if anything.
    TellOrchestrator {
        chat: String,
        resume: OrchestratorResume,
    },
    /// Continues an `interrupted` task (sent by the runtime after `Restart`).
    ResumeTask {
        task: String,
    },
    /// The user retries the base merge of a `merge_blocked` group (Stage 5).
    RetryGroupMerge {
        group: String,
    },
}

/// What happened to a chat's orchestrator session when it is started again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestratorResume {
    /// There was a session before.
    pub had_session: bool,
    /// That session can be restored with `session/load` (same working directory).
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
    /// Deliver the orchestrator's `message_task` message to the agent of the
    /// running step: steer it into the running turn, or interrupt the turn and
    /// send it as the next prompt (`core-design.md` §8). The runtime settles it
    /// with [`DomainCommand::MessageResolved`].
    DeliverMessage {
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
    /// The inbox of `chat` has new entries; deliver them when its orchestrator
    /// is idle (starting it if it is not running).
    WakeOrchestrator {
        chat: String,
    },
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
