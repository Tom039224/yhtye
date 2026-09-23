//! Domain events (facts) and the pure reducer that applies them to [`State`].
//!
//! Every state change goes through [`State::apply`]. The decision logic
//! (`machine.rs`) never mutates state directly; it emits events and applies them
//! to a working copy. Replaying the events of a transition on the old state
//! therefore always yields the new state, which is what persistence (Stage 3b)
//! relies on.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::state::{
    Group, GroupStatus, Help, HelpState, InboxEntry, State, Step, StepStatus, Task, TaskStatus,
};
use super::types::{StepSpec, Verdict};
use crate::mcp::tools::HelpAction;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEvent {
    GroupCreated {
        group: Group,
    },
    GroupFinishing {
        group: String,
        summary: String,
    },
    /// The orchestrator ended its turn leaving a settled group open; it was
    /// reminded to finish it (Stage 7a).
    GroupFinishReminded {
        group: String,
    },
    /// The group branch was merged into the base branch (`ok`) or could not be.
    GroupMergeFinished {
        group: String,
        ok: bool,
        detail: String,
    },
    GroupCancelled {
        group: String,
        reason: String,
    },
    TaskCreated {
        task: Task,
    },
    InstructionSet {
        task: String,
        instruction: String,
    },
    TaskStatusChanged {
        task: String,
        status: TaskStatus,
    },
    TaskCancelled {
        task: String,
        reason: String,
    },
    WorkspaceReady {
        task: String,
        path: PathBuf,
    },
    StepStarted {
        task: String,
        step: usize,
        note: Option<String>,
    },
    StepCompleted {
        task: String,
        step: usize,
        result: String,
        verdict: Option<Verdict>,
    },
    /// A step goes back to pending (e.g. `done` after a merge conflict).
    StepReset {
        task: String,
        step: usize,
    },
    /// `modify_steps`: every step from `from` on is replaced.
    StepsReplaced {
        task: String,
        from: usize,
        steps: Vec<StepSpec>,
    },
    /// A `needs_changes` review inserted `steps` right after step `after`.
    ReviewStepsInserted {
        task: String,
        after: usize,
        steps: Vec<StepSpec>,
    },
    NudgeSent {
        task: String,
        step: usize,
    },
    HelpRaised {
        help: Help,
    },
    HelpAnswered {
        help: String,
        action: HelpAction,
        reply: Option<String>,
    },
    HelpClosed {
        help: String,
    },
    HelpAgentLost {
        help: String,
    },
    InboxQueued {
        entry: InboxEntry,
    },
    /// Every inbox entry with `id <= up_to` was sent to the orchestrator.
    InboxDelivered {
        up_to: u64,
    },
}

impl State {
    /// Applies one event. Never fails: an event naming an unknown id changes nothing
    /// (it cannot be produced by the state machine for this state).
    pub fn apply(&mut self, event: &DomainEvent) {
        match event {
            DomainEvent::GroupCreated { .. }
            | DomainEvent::GroupFinishing { .. }
            | DomainEvent::GroupFinishReminded { .. }
            | DomainEvent::GroupMergeFinished { .. }
            | DomainEvent::GroupCancelled { .. } => self.apply_group(event),
            DomainEvent::HelpRaised { .. }
            | DomainEvent::HelpAnswered { .. }
            | DomainEvent::HelpClosed { .. }
            | DomainEvent::HelpAgentLost { .. } => self.apply_help(event),
            DomainEvent::InboxQueued { entry } => {
                self.counters.inbox = self.counters.inbox.max(entry.id);
                self.inbox.push(entry.clone());
            }
            DomainEvent::InboxDelivered { up_to } => self.inbox.retain(|e| e.id > *up_to),
            DomainEvent::TaskCreated { task } => {
                self.counters.tasks += 1;
                self.tasks.push(task.clone());
            }
            _ => self.apply_task(event),
        }
    }

    fn apply_group(&mut self, event: &DomainEvent) {
        match event {
            DomainEvent::GroupCreated { group } => {
                self.counters.groups += 1;
                self.groups.push(group.clone());
            }
            DomainEvent::GroupFinishing { group, summary } => {
                if let Some(g) = self.group_mut(group) {
                    g.status = GroupStatus::Finishing;
                    g.finish_summary = Some(summary.clone());
                }
            }
            DomainEvent::GroupFinishReminded { group } => {
                if let Some(g) = self.group_mut(group) {
                    g.finish_nudges += 1;
                }
            }
            DomainEvent::GroupMergeFinished { group, ok, detail } => {
                if let Some(g) = self.group_mut(group) {
                    g.status = if *ok {
                        GroupStatus::Done
                    } else {
                        GroupStatus::MergeBlocked
                    };
                    g.detail = Some(detail.clone());
                }
            }
            DomainEvent::GroupCancelled { group, reason } => {
                if let Some(g) = self.group_mut(group) {
                    g.status = GroupStatus::Cancelled;
                    g.detail = Some(reason.clone());
                }
            }
            _ => {}
        }
    }

    fn apply_help(&mut self, event: &DomainEvent) {
        match event {
            DomainEvent::HelpRaised { help } => {
                self.counters.helps += 1;
                self.helps.push(help.clone());
            }
            DomainEvent::HelpAnswered { help, reply, .. } => {
                let Some(h) = self.help_mut(help) else { return };
                h.state = HelpState::Answered;
                h.reply = reply.clone();
                let (task, step) = (h.task.clone(), h.step);
                // The answer is a new prompt: the agent gets a fresh reminder budget.
                if let Some(s) = self.task_mut(&task).and_then(|t| t.steps.get_mut(step)) {
                    s.nudges = 0;
                }
            }
            DomainEvent::HelpClosed { help } => {
                if let Some(h) = self.help_mut(help) {
                    h.state = HelpState::Closed;
                }
            }
            DomainEvent::HelpAgentLost { help } => {
                if let Some(h) = self.help_mut(help) {
                    h.agent_lost = true;
                }
            }
            _ => {}
        }
    }

    fn apply_task(&mut self, event: &DomainEvent) {
        match event {
            DomainEvent::InstructionSet { task, instruction } => {
                self.with_task(task, |t| t.instruction = Some(instruction.clone()));
            }
            DomainEvent::TaskStatusChanged { task, status } => {
                self.with_task(task, |t| t.status = *status);
            }
            DomainEvent::TaskCancelled { task, reason } => self.with_task(task, |t| {
                t.status = TaskStatus::Cancelled;
                t.cancel_reason = Some(reason.clone());
            }),
            DomainEvent::WorkspaceReady { task, path } => {
                self.with_task(task, |t| t.workdir = Some(path.clone()));
            }
            DomainEvent::StepsReplaced { task, from, steps } => self.with_task(task, |t| {
                t.steps.truncate(*from);
                t.steps.extend(steps.iter().map(Step::from_spec));
            }),
            DomainEvent::ReviewStepsInserted { task, after, steps } => self.with_task(task, |t| {
                let at = (*after + 1).min(t.steps.len());
                t.steps.splice(at..at, steps.iter().map(Step::from_spec));
                t.review_rounds += 1;
            }),
            _ => self.apply_step(event),
        }
    }

    fn apply_step(&mut self, event: &DomainEvent) {
        match event {
            DomainEvent::StepStarted { task, step, note } => self.with_task(task, |t| {
                t.current = *step;
                if let Some(s) = t.steps.get_mut(*step) {
                    s.status = StepStatus::Running;
                    s.note = note.clone();
                    s.nudges = 0;
                }
            }),
            DomainEvent::StepCompleted {
                task,
                step,
                result,
                verdict,
            } => self.with_step(task, *step, |s| {
                s.status = StepStatus::Done;
                s.result = Some(result.clone());
                s.verdict = *verdict;
            }),
            DomainEvent::StepReset { task, step } => {
                self.with_step(task, *step, |s| s.status = StepStatus::Pending);
            }
            DomainEvent::NudgeSent { task, step } => self.with_step(task, *step, |s| s.nudges += 1),
            _ => {}
        }
    }

    fn with_task(&mut self, id: &str, f: impl FnOnce(&mut Task)) {
        if let Some(t) = self.task_mut(id) {
            f(t);
        }
    }

    fn with_step(&mut self, task: &str, step: usize, f: impl FnOnce(&mut Step)) {
        if let Some(s) = self.task_mut(task).and_then(|t| t.steps.get_mut(step)) {
            f(s);
        }
    }
}
