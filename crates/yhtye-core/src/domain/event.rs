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
    Chat, Group, GroupStatus, Help, HelpState, InboxEntry, State, Step, StepStatus, Task,
    TaskStatus,
};
use super::types::{StepSpec, Verdict};
use crate::mcp::tools::HelpAction;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEvent {
    /// A chat was created (Stage 8, `orchestration-model.md` §2.0).
    ChatCreated {
        chat: Chat,
    },
    /// The chat got its title: from its first user message, or the user's own
    /// (`rename_chat`).
    ChatTitled {
        chat: String,
        title: String,
    },
    /// The user deleted the chat. Its groups, their tasks and helps, and its
    /// inbox go with it; the counters stay (ids are never reused).
    ChatDeleted {
        chat: String,
    },
    GroupCreated {
        group: Group,
    },
    /// `finish_group` with `into` (Stage 8e): the group now merges into `to`,
    /// the branch its chat's worktree has checked out.
    GroupBaseChanged {
        group: String,
        from: String,
        to: String,
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
    /// Every inbox entry of `chat` with `id <= up_to` was sent to its orchestrator.
    InboxDelivered {
        chat: String,
        up_to: u64,
    },
}

impl State {
    /// Applies one event. Never fails: an event naming an unknown id changes nothing
    /// (it cannot be produced by the state machine for this state).
    pub fn apply(&mut self, event: &DomainEvent) {
        match event {
            DomainEvent::GroupCreated { .. }
            | DomainEvent::GroupBaseChanged { .. }
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
            DomainEvent::InboxDelivered { chat, up_to } => {
                self.inbox.retain(|e| e.chat != *chat || e.id > *up_to);
            }
            DomainEvent::ChatCreated { chat } => {
                self.counters.chats += 1;
                self.chats.push(chat.clone());
            }
            DomainEvent::ChatTitled { chat, title } => {
                if let Some(c) = self.chats.iter_mut().find(|c| c.id == *chat) {
                    c.title = Some(title.clone());
                }
            }
            DomainEvent::ChatDeleted { chat } => self.remove_chat(chat),
            DomainEvent::TaskCreated { task } => {
                self.counters.tasks += 1;
                self.tasks.push(task.clone());
            }
            _ => self.apply_task(event),
        }
    }

    /// Drops `chat` with everything that belongs to it.
    fn remove_chat(&mut self, chat: &str) {
        let groups: Vec<String> = self
            .groups
            .iter()
            .filter(|g| g.chat == chat)
            .map(|g| g.id.clone())
            .collect();
        let tasks: Vec<String> = self
            .tasks
            .iter()
            .filter(|t| groups.contains(&t.group))
            .map(|t| t.id.clone())
            .collect();
        self.chats.retain(|c| c.id != chat);
        self.groups.retain(|g| g.chat != chat);
        self.tasks.retain(|t| !groups.contains(&t.group));
        self.helps.retain(|h| !tasks.contains(&h.task));
        self.inbox.retain(|e| e.chat != chat);
    }

    fn apply_group(&mut self, event: &DomainEvent) {
        match event {
            DomainEvent::GroupCreated { group } => {
                // The id may be past the counter (numbers taken in git).
                let number = group.id.strip_prefix("G-").and_then(|n| n.parse().ok());
                self.counters.groups = number.map_or(self.counters.groups + 1, |n: u32| {
                    n.max(self.counters.groups + 1)
                });
                self.groups.push(group.clone());
            }
            DomainEvent::GroupBaseChanged { group, to, .. } => {
                if let Some(g) = self.group_mut(group) {
                    g.base_branch = to.clone();
                }
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
