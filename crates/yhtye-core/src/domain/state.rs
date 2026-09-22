//! The orchestration state (`orchestration-model.md` §2, §4). Plain data: it is
//! changed only by [`super::DomainEvent`]s through [`State::apply`] (see `event.rs`),
//! so it can be rebuilt from an event log or stored as current-state tables.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::inbox::InboxItem;
use super::types::{HelpKind, Role, StepKind, StepSpec, TaskKind, Verdict};

/// Default `max_review_rounds` (`orchestration-model.md` §2.3).
pub const DEFAULT_MAX_REVIEW_ROUNDS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub title: String,
    pub summary: Option<String>,
    pub base_branch: String,
    pub group_branch: String,
    pub status: GroupStatus,
    /// `finish_group`'s summary for the user.
    pub finish_summary: Option<String>,
    /// Result of the merge into the base branch, or why the group was cancelled.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum HelpSource {
    /// The agent of `role` working on the help's step (via the `help` tool).
    Agent { role: Role },
    /// Yhtye itself (merge conflict, protocol violation, crash, ...).
    Yhtye,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelpState {
    Open,
    Answered,
    /// The task ended (cancelled) before the help was answered.
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxEntry {
    pub id: u64,
    pub item: InboxItem,
}

/// Id counters (the number of each kind created so far).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counters {
    pub groups: u32,
    pub tasks: u32,
    pub helps: u32,
    pub inbox: u64,
}

/// Everything the state machine knows about one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub project: String,
    pub config: DomainConfig,
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
            groups: Vec::new(),
            tasks: Vec::new(),
            helps: Vec::new(),
            inbox: Vec::new(),
            counters: Counters::default(),
        }
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

    /// The group that blocks creating another one (`active` or `finishing`).
    #[must_use]
    pub fn open_group(&self) -> Option<&Group> {
        self.groups
            .iter()
            .find(|g| matches!(g.status, GroupStatus::Active | GroupStatus::Finishing))
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
