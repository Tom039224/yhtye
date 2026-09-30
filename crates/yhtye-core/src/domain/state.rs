//! The orchestration state (`orchestration-model.md` §2, §4). Plain data: it is
//! changed only by [`super::DomainEvent`]s through [`State::apply`] (see `event.rs`),
//! so it can be rebuilt from an event log or stored as current-state tables.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::inbox::InboxItem;
use super::types::{HelpKind, Role, StepKind, StepSpec, TaskKind, Verdict};
use crate::agents::AgentChoice;

/// Default `max_review_rounds` (`orchestration-model.md` §2.3).
pub const DEFAULT_MAX_REVIEW_ROUNDS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DomainConfig {
    /// How many times a `needs_changes` review may insert implement + review.
    pub max_review_rounds: u32,
}

impl Default for DomainConfig {
    fn default() -> Self {
        Self {
            max_review_rounds: DEFAULT_MAX_REVIEW_ROUNDS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum GroupStatus {
    Active,
    Finishing,
    Done,
    Cancelled,
    MergeBlocked,
}

impl GroupStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Finishing => "finishing",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
            Self::MergeBlocked => "merge_blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    AwaitingInstruction,
    Running,
    Checkpoint,
    Handling,
    Merging,
    /// Set on restart for tasks that were mid-turn or mid-merge (Stage 3b).
    Interrupted,
    Done,
    Cancelled,
}

impl TaskStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::AwaitingInstruction => "awaiting_instruction",
            Self::Running => "running",
            Self::Checkpoint => "checkpoint",
            Self::Handling => "handling",
            Self::Merging => "merging",
            Self::Interrupted => "interrupted",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
}

/// Branch of group `group`: `yhtye/<groupId>` (`orchestration-model.md` §6).
#[must_use]
pub fn group_branch(group: &str) -> String {
    format!("yhtye/{group}")
}

/// Branch of `code` task `task` in group `group`: `yhtye/<groupId>-<taskId>`.
/// Not `yhtye/<groupId>/<taskId>`: git cannot have both `refs/heads/yhtye/G-1`
/// and `refs/heads/yhtye/G-1/T-1` (a ref cannot also be a directory).
#[must_use]
pub fn task_branch(group: &str, task: &str) -> String {
    format!("yhtye/{group}-{task}")
}

/// A conversation with the orchestrator, bound to one worktree
/// (`orchestration-model.md` §2.0, Stage 8e). The branch is not part of the
/// chat: it is read from the worktree when shown, and when a group is created
/// or merged. Times are not part of the state (the store derives them from
/// the event log).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Chat {
    /// `C-<n>`.
    pub id: String,
    /// The worktree the chat works in, as `git worktree list` shows it: its
    /// orchestrator's working directory and where its groups are merged. It
    /// never changes.
    pub worktree: PathBuf,
    /// The first user message, shortened (`None` until it is sent).
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Group {
    pub id: String,
    /// The chat that created the group (`C-<n>`).
    pub chat: String,
    pub title: String,
    pub summary: Option<String>,
    /// Where the group is merged: the branch the chat's worktree had checked
    /// out when the group was created (Stage 8e), or `finish_group`'s `into`.
    pub base_branch: String,
    pub group_branch: String,
    pub status: GroupStatus,
    /// `finish_group`'s summary for the user.
    pub finish_summary: Option<String>,
    /// Result of the merge into the base branch, or why the group was cancelled.
    pub detail: Option<String>,
    /// Reminders sent because the orchestrator ended its turn while every task
    /// had settled and the group was still open (Stage 7a; 0 in older logs).
    #[serde(default)]
    pub finish_nudges: u32,
}

impl Group {
    /// `active` or `finishing`: counts against the branch's open-group limit.
    #[must_use]
    pub fn is_open(&self) -> bool {
        matches!(self.status, GroupStatus::Active | GroupStatus::Finishing)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Step {
    pub kind: StepKind,
    pub instruction: Option<String>,
    pub status: StepStatus,
    pub result: Option<String>,
    pub verdict: Option<Verdict>,
    /// Note from the orchestrator given when the step (re)started.
    pub note: Option<String>,
    /// Reminders sent because a turn ended without `report_step_done` / `help`.
    pub nudges: u32,
}

impl Step {
    #[must_use]
    pub fn from_spec(spec: &StepSpec) -> Self {
        Self {
            kind: spec.kind,
            instruction: spec.instruction.clone(),
            status: StepStatus::Pending,
            result: None,
            verdict: None,
            note: None,
            nudges: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Task {
    pub id: String,
    pub group: String,
    pub title: String,
    pub kind: TaskKind,
    pub depends_on: Vec<String>,
    pub instruction: Option<String>,
    pub steps: Vec<Step>,
    /// Index of the step being run, or the next one to run.
    pub current: usize,
    pub status: TaskStatus,
    pub review_rounds: u32,
    /// Where the task's agents work (set once the git workspace is ready).
    pub workdir: Option<PathBuf>,
    pub cancel_reason: Option<String>,
    /// The orchestrator's choice of harness × model for the task's agent
    /// (`create_task` `harness` / `model`, Stage 7b); `None` = the role default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent: Option<AgentChoice>,
    /// The same for the task's review steps (`review_harness` / `review_model`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub review_agent: Option<AgentChoice>,
}

impl Task {
    /// Index of the first step that is not done (where a resumed task continues).
    #[must_use]
    pub fn first_unfinished(&self) -> usize {
        self.steps
            .iter()
            .position(|s| s.status != StepStatus::Done)
            .unwrap_or(self.steps.len())
    }

    /// Index from which all steps are still pending (what `modify_steps` replaces).
    #[must_use]
    pub fn pending_tail(&self) -> usize {
        self.steps
            .iter()
            .rposition(|s| s.status != StepStatus::Pending)
            .map_or(0, |i| i + 1)
    }

    /// Result of the last non-`done` step that has one (what the task produced;
    /// the `done` step only records the merge).
    #[must_use]
    pub fn final_result(&self) -> Option<&str> {
        self.steps
            .iter()
            .rev()
            .filter(|s| s.kind != StepKind::Done)
            .find_map(|s| s.result.as_deref())
    }

    #[must_use]
    pub fn current_step(&self) -> Option<&Step> {
        self.steps.get(self.current)
    }

    /// Git branch of a `code` task (`orchestration-model.md` §6).
    #[must_use]
    pub fn branch(&self) -> Option<String> {
        (self.kind == TaskKind::Code).then(|| task_branch(&self.group, &self.id))
    }
}

/// Who raised a help.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum HelpSource {
    /// The agent of `role` working on the help's step (via the `help` tool).
    Agent { role: Role },
    /// Yhtye itself (merge conflict, protocol violation, crash, ...).
    Yhtye,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HelpState {
    Open,
    Answered,
    /// The task ended (cancelled) before the help was answered.
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Help {
    pub id: String,
    pub task: String,
    pub step: usize,
    pub kind: HelpKind,
    pub message: String,
    pub source: HelpSource,
    pub state: HelpState,
    /// The asking agent's process died while waiting for the answer.
    pub agent_lost: bool,
    pub reply: Option<String>,
}

impl Help {
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.state == HelpState::Open
    }
}

/// An undelivered reason to wake the orchestrator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InboxEntry {
    pub id: u64,
    /// The chat whose orchestrator this is for.
    pub chat: String,
    pub item: InboxItem,
}

/// Id counters (the number of each kind created so far).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Counters {
    #[serde(default)]
    pub chats: u32,
    pub groups: u32,
    pub tasks: u32,
    pub helps: u32,
    pub inbox: u64,
}

/// Everything the state machine knows about one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct State {
    pub project: String,
    pub config: DomainConfig,
    /// In creation order.
    pub chats: Vec<Chat>,
    pub groups: Vec<Group>,
    pub tasks: Vec<Task>,
    pub helps: Vec<Help>,
    /// Undelivered inbox entries, oldest first.
    pub inbox: Vec<InboxEntry>,
    pub counters: Counters,
}

impl State {
    #[must_use]
    pub fn new(project: impl Into<String>, config: DomainConfig) -> Self {
        Self {
            project: project.into(),
            config,
            chats: Vec::new(),
            groups: Vec::new(),
            tasks: Vec::new(),
            helps: Vec::new(),
            inbox: Vec::new(),
            counters: Counters::default(),
        }
    }

    #[must_use]
    pub fn chat(&self, id: &str) -> Option<&Chat> {
        self.chats.iter().find(|c| c.id == id)
    }

    /// The chat that created `group`.
    #[must_use]
    pub fn chat_of_group(&self, group: &str) -> Option<&str> {
        self.group(group).map(|g| g.chat.as_str())
    }

    /// The chat of the group `task` belongs to.
    #[must_use]
    pub fn chat_of_task(&self, task: &str) -> Option<&str> {
        self.chat_of_group(&self.task(task)?.group)
    }

    #[must_use]
    pub fn group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    #[must_use]
    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    #[must_use]
    pub fn help(&self, id: &str) -> Option<&Help> {
        self.helps.iter().find(|h| h.id == id)
    }

    pub(super) fn group_mut(&mut self, id: &str) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    pub(super) fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub(super) fn help_mut(&mut self, id: &str) -> Option<&mut Help> {
        self.helps.iter_mut().find(|h| h.id == id)
    }

    /// The worktree of the chat that created `group`.
    #[must_use]
    pub fn worktree_of_group(&self, group: &str) -> Option<&Path> {
        let chat = self.chat_of_group(group)?;
        self.chat(chat).map(|c| c.worktree.as_path())
    }

    /// The group in `worktree` that blocks opening another one there (`active`
    /// or `finishing`; any chat's in that worktree, Stage 8e).
    #[must_use]
    pub fn open_group_in(&self, worktree: &Path) -> Option<&Group> {
        self.groups
            .iter()
            .find(|g| g.is_open() && self.worktree_of_group(&g.id) == Some(worktree))
    }

    /// The `active` or `finishing` group of `chat` (what `get_status` shows by default).
    #[must_use]
    pub fn open_group_of(&self, chat: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.chat == chat && g.is_open())
    }

    /// Whether any group is `active` or `finishing`.
    #[must_use]
    pub fn has_open_group(&self) -> bool {
        self.groups.iter().any(Group::is_open)
    }

    /// The undelivered inbox entries of `chat`, oldest first.
    pub fn inbox_of<'a>(&'a self, chat: &'a str) -> impl Iterator<Item = &'a InboxEntry> + 'a {
        self.inbox.iter().filter(move |e| e.chat == chat)
    }

    pub fn tasks_of<'a>(&'a self, group: &'a str) -> impl Iterator<Item = &'a Task> + 'a {
        self.tasks.iter().filter(move |t| t.group == group)
    }

    /// The unanswered help of `task`, if any (at most one exists).
    #[must_use]
    pub fn open_help(&self, task: &str) -> Option<&Help> {
        self.helps.iter().find(|h| h.task == task && h.is_open())
    }

    /// A pending task that can never start because a dependency was cancelled
    /// (directly or through another such task).
    #[must_use]
    pub fn is_blocked(&self, task: &Task) -> bool {
        self.blocked_depth(task, 0)
    }

    fn blocked_depth(&self, task: &Task, depth: usize) -> bool {
        // Dependencies always point to older tasks, so there are no cycles; the
        // depth bound only guards against corrupted state.
        if task.status != TaskStatus::Pending || depth > self.tasks.len() {
            return false;
        }
        task.depends_on
            .iter()
            .filter_map(|d| self.task(d))
            .any(|d| d.status == TaskStatus::Cancelled || self.blocked_depth(d, depth + 1))
    }

    /// Whether every dependency of `task` is done.
    #[must_use]
    pub fn deps_done(&self, task: &Task) -> bool {
        task.depends_on
            .iter()
            .all(|d| self.task(d).is_some_and(|d| d.status == TaskStatus::Done))
    }
}
