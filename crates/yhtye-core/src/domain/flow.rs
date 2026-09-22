//! Task lifecycle shared by all commands: starting tasks and steps, finishing and
//! cancelling tasks, raising help, dependency resolution and group settlement
//! (`orchestration-model.md` §2–§5).

use super::command::{AgentRef, Effect, GitOp};
use super::event::DomainEvent;
use super::inbox::{InboxItem, InboxKind};
use super::machine::Tx;
use super::state::{GroupStatus, Help, HelpSource, HelpState, StepStatus, Task, TaskStatus};
use super::types::{HelpKind, StepKind, TaskKind};
use crate::prompts::{StepPrompt, step_prompt};

impl Tx {
    /// Starts `id` if its dependencies are done: runs it when it has an
    /// instruction, otherwise asks the orchestrator for one (`instruction_needed`).
    pub(super) fn try_start(&mut self, id: &str) {
        let Some(t) = self.state.task(id) else { return };
        let waiting = matches!(
            t.status,
            TaskStatus::Pending | TaskStatus::AwaitingInstruction
        );
        if !waiting || !self.state.deps_done(t) {
            return;
        }
        if t.instruction.is_some() {
            return self.begin_task(id, None);
        }
        if t.status == TaskStatus::AwaitingInstruction {
            return;
        }
        let body = dependency_results(&self.state, t);
        self.set_status(id, TaskStatus::AwaitingInstruction);
        self.queue_inbox(InboxItem::new(
            InboxKind::InstructionNeeded,
            &[("task", id)],
            body,
        ));
    }

    /// Runs a task from its first unfinished step, preparing its git workspace
    /// first if it has none yet (the step starts when the workspace is ready).
    pub(super) fn begin_task(&mut self, id: &str, note: Option<String>) {
        let Some(t) = self.state.task(id) else { return };
        if t.workdir.is_none() {
            let (group_branch, base_branch) = self.branches_of(&t.group);
            let op = GitOp::PrepareWorkspace {
                group: t.group.clone(),
                task: t.id.clone(),
                kind: t.kind,
                group_branch,
                base_branch,
            };
            self.set_status(id, TaskStatus::Running);
            return self.effect(Effect::Git(op));
        }
        let index = t.first_unfinished();
        self.start_step(id, index, note);
    }

    /// Starts step `index` of task `id` according to its kind.
    pub(super) fn start_step(&mut self, id: &str, index: usize, note: Option<String>) {
        let Some(kind) = self
            .state
            .task(id)
            .and_then(|t| t.steps.get(index))
            .map(|s| s.kind)
        else {
            tracing::error!("task {id} has no step {index}; finishing it");
            return self.complete_task(id);
        };
        self.emit(DomainEvent::StepStarted {
            task: id.to_string(),
            step: index,
            note,
        });
        match kind {
            StepKind::Implement | StepKind::Review => self.run_agent_step(id, index),
            StepKind::Checkpoint => self.reach_checkpoint(id, index),
            StepKind::Done => self.start_finish(id),
        }
    }

    /// Starts the step after the current one.
    pub(super) fn advance(&mut self, id: &str) {
        if let Some(t) = self.state.task(id) {
            let next = t.current + 1;
            self.start_step(id, next, None);
        }
    }

    fn run_agent_step(&mut self, id: &str, index: usize) {
        self.set_status(id, TaskStatus::Running);
        let Some(t) = self.state.task(id) else { return };
        let Some(role) = t.steps.get(index).and_then(|s| s.kind.role()) else {
            return;
        };
        let effect = Effect::RunStep {
            agent: AgentRef {
                task: t.id.clone(),
                role,
                step: index,
            },
            group: t.group.clone(),
            prompt: render_step_prompt(t, index),
            workdir: t.workdir.clone(),
        };
        self.effect(effect);
    }

    fn reach_checkpoint(&mut self, id: &str, index: usize) {
        self.set_status(id, TaskStatus::Checkpoint);
        let body = self
            .state
            .task(id)
            .map(|t| earlier_results(t, index))
            .unwrap_or_default();
        let step = index.to_string();
        self.queue_inbox(InboxItem::new(
            InboxKind::CheckpointReached,
            &[("task", id), ("step", &step)],
            body,
        ));
    }

    pub(super) fn start_finish(&mut self, id: &str) {
        self.set_status(id, TaskStatus::Merging);
        let Some(t) = self.state.task(id) else { return };
        let (group_branch, base_branch) = self.branches_of(&t.group);
        let op = GitOp::FinishTask {
            group: t.group.clone(),
            task: t.id.clone(),
            kind: t.kind,
            message: format!(
                "{}: {}\n\n{}",
                t.id,
                t.title,
                t.final_result().unwrap_or("(no result)")
            ),
            group_branch,
            base_branch,
        };
        self.effect(Effect::Git(op));
    }

    /// `(group_branch, base_branch)` of `group`.
    fn branches_of(&self, group: &str) -> (String, String) {
        self.state.group(group).map_or_else(
            || (super::state::group_branch(group), String::new()),
            |g| (g.group_branch.clone(), g.base_branch.clone()),
        )
    }

    /// The task finished successfully: stop its agents, start dependents, settle.
    pub(super) fn complete_task(&mut self, id: &str) {
        self.set_status(id, TaskStatus::Done);
        self.effect(Effect::StopTaskAgents {
            task: id.to_string(),
        });
        let Some(group) = self.state.task(id).map(|t| t.group.clone()) else {
            return;
        };
        self.start_dependents(&group);
        self.settle_check(&group);
    }

    /// Cancels a non-terminal task: closes its help, stops its agents and removes
    /// its worktree (the branch is kept). Dependents are not cancelled.
    pub(super) fn cancel_task(&mut self, id: &str, reason: &str) {
        let Some(t) = self.state.task(id) else { return };
        if t.status.is_terminal() {
            return;
        }
        let group = t.group.clone();
        let remove =
            (t.kind == TaskKind::Code && t.workdir.is_some()).then(|| GitOp::RemoveWorkspace {
                group: group.clone(),
                task: id.to_string(),
            });
        if let Some(help) = self.state.open_help(id).map(|h| h.id.clone()) {
            self.emit(DomainEvent::HelpClosed { help });
        }
        self.emit(DomainEvent::TaskCancelled {
            task: id.to_string(),
            reason: reason.to_string(),
        });
        self.effect(Effect::StopTaskAgents {
            task: id.to_string(),
        });
        if let Some(op) = remove {
            self.effect(Effect::Git(op));
        }
        self.settle_check(&group);
    }

    /// Raises a help on the task's current step, moves it to `handling` and wakes
    /// the orchestrator. Returns the help id (an already open help is kept).
    pub(super) fn raise_help(
        &mut self,
        id: &str,
        kind: HelpKind,
        message: &str,
        source: HelpSource,
    ) -> Option<String> {
        if let Some(open) = self.state.open_help(id) {
            tracing::warn!(
                "{id} already has open help {}; not raising {kind:?}",
                open.id
            );
            return Some(open.id.clone());
        }
        let step = self.state.task(id)?.current;
        let help_id = format!("H-{}", self.state.counters.helps + 1);
        self.emit(DomainEvent::HelpRaised {
            help: Help {
                id: help_id.clone(),
                task: id.to_string(),
                step,
                kind,
                message: message.to_string(),
                source,
                state: HelpState::Open,
                agent_lost: false,
                reply: None,
            },
        });
        self.set_status(id, TaskStatus::Handling);
        self.queue_inbox(InboxItem::new(
            InboxKind::HelpRaised,
            &[("help_id", &help_id), ("task", id), ("kind", kind.as_str())],
            message,
        ));
        Some(help_id)
    }

    /// Help raised by Yhtye itself.
    pub(super) fn raise_yhtye_help(&mut self, id: &str, kind: HelpKind, message: &str) {
        self.raise_help(id, kind, message, HelpSource::Yhtye);
    }

    pub(super) fn set_status(&mut self, id: &str, status: TaskStatus) {
        if self.state.task(id).is_some_and(|t| t.status != status) {
            self.emit(DomainEvent::TaskStatusChanged {
                task: id.to_string(),
                status,
            });
        }
    }

    fn start_dependents(&mut self, group: &str) {
        let waiting: Vec<String> = self
            .state
            .tasks_of(group)
            .filter(|t| t.status == TaskStatus::Pending)
            .map(|t| t.id.clone())
            .collect();
        for id in waiting {
            self.try_start(&id);
        }
    }

    /// Wakes the orchestrator with `group_settled` once every task of an active
    /// group is terminal or can never start.
    pub(super) fn settle_check(&mut self, group: &str) {
        let active = self
            .state
            .group(group)
            .is_some_and(|g| g.status == GroupStatus::Active);
        if !active {
            return;
        }
        let mut lines = Vec::new();
        for t in self.state.tasks_of(group) {
            if t.status.is_terminal() {
                let result = t.final_result().unwrap_or("(no result)");
                lines.push(format!("{} {}: {result}", t.id, t.status.as_str()));
            } else if self.state.is_blocked(t) {
                lines.push(format!(
                    "{} cannot start (a dependency was cancelled)",
                    t.id
                ));
            } else {
                return;
            }
        }
        if lines.is_empty() {
            return;
        }
        self.queue_inbox(InboxItem::new(
            InboxKind::GroupSettled,
            &[("group", group)],
            lines.join("\n"),
        ));
    }
}

/// The session of the agent that asked for help (for resuming it).
pub(super) fn help_agent(help: &Help) -> Option<AgentRef> {
    match help.source {
        HelpSource::Agent { role } => Some(AgentRef {
            task: help.task.clone(),
            role,
            step: help.step,
        }),
        HelpSource::Yhtye => None,
    }
}

/// Whether `agent` is the session running the task's current step.
pub(super) fn runs_current_step(t: &Task, agent: &AgentRef) -> bool {
    agent.step == t.current
        && t.current_step()
            .is_some_and(|s| s.kind.role() == Some(agent.role))
}

/// The prompt that starts step `index`.
pub(super) fn render_step_prompt(t: &Task, index: usize) -> String {
    let Some(step) = t.steps.get(index) else {
        return String::new();
    };
    let instruction = step
        .instruction
        .as_deref()
        .or(t.instruction.as_deref())
        .unwrap_or_default();
    let context = (step.kind == StepKind::Review).then(|| earlier_results(t, index));
    let branches = t
        .branch()
        .map(|b| (b, super::state::group_branch(&t.group)));
    let review_range = branches
        .as_ref()
        .filter(|_| step.kind == StepKind::Review)
        .map(|(task, group)| (task.as_str(), group.as_str()));
    step_prompt(&StepPrompt {
        task_id: &t.id,
        task_title: &t.title,
        task_kind: t.kind,
        step_index: index,
        step_count: t.steps.len(),
        step_kind: step.kind,
        instruction,
        context: context.as_deref().filter(|c| !c.is_empty()),
        note: step.note.as_deref(),
        review_range,
    })
}

/// Results of the steps before `index`, one per line.
fn earlier_results(t: &Task, index: usize) -> String {
    t.steps
        .iter()
        .take(index)
        .enumerate()
        .filter(|(_, s)| s.status == StepStatus::Done)
        .filter_map(|(i, s)| {
            let r = s.result.as_ref()?;
            Some(format!("step {i} ({}): {r}", step_kind_label(s.kind)))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn step_kind_label(kind: StepKind) -> &'static str {
    match kind {
        StepKind::Implement => "implement",
        StepKind::Review => "review",
        StepKind::Checkpoint => "checkpoint",
        StepKind::Done => "done",
    }
}

fn dependency_results(state: &super::state::State, t: &Task) -> String {
    t.depends_on
        .iter()
        .filter_map(|d| state.task(d))
        .map(|d| format!("{}: {}", d.id, d.final_result().unwrap_or("(no result)")))
        .collect::<Vec<_>>()
        .join("\n")
}
