//! Restarting on a stored state (`orchestration-model.md` §10, Stage 3b).
//!
//! `Restart` marks what was in flight when Yhtye stopped; `ResumeTask` then
//! continues each `interrupted` task. Tasks waiting for the orchestrator
//! (`checkpoint` / `handling` / `awaiting_instruction`) stay as they are; the
//! undelivered inbox is delivered again by the runtime.

use super::command::{AgentRef, Effect, OrchestratorResume};
use super::event::DomainEvent;
use super::flow::{help_agent, render_step_prompt};
use super::inbox::{InboxItem, InboxKind};
use super::machine::Tx;
use super::state::{GroupStatus, State, StepStatus, TaskStatus};
use super::types::StepKind;
use crate::prompts::{RESTART_NOTE, resume_prompt};

impl Tx {
    pub(super) fn restart(&mut self, orchestrator: OrchestratorResume) {
        let in_flight: Vec<String> = self
            .state
            .tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Running | TaskStatus::Merging))
            .map(|t| t.id.clone())
            .collect();
        for task in in_flight {
            self.set_status(&task, TaskStatus::Interrupted);
        }
        self.mark_waiting_agents_lost();
        self.block_interrupted_merges();
        self.tell_orchestrator(orchestrator);
    }

    /// Agents waiting for a help answer died with Yhtye: resuming the help
    /// restarts the step (their session is restored if possible).
    fn mark_waiting_agents_lost(&mut self) {
        let lost: Vec<String> = self
            .state
            .helps
            .iter()
            .filter(|h| h.is_open() && !h.agent_lost && help_agent(h).is_some())
            .map(|h| h.id.clone())
            .collect();
        for help in lost {
            self.emit(DomainEvent::HelpAgentLost { help });
        }
    }

    /// A group merge cannot be half done in the log (git runs inside one command
    /// chain), but Yhtye may have died during the git command itself. The group is
    /// reported as `merge_blocked` so the merge can be retried after a check.
    fn block_interrupted_merges(&mut self) {
        let finishing: Vec<String> = self
            .state
            .groups
            .iter()
            .filter(|g| g.status == GroupStatus::Finishing)
            .map(|g| g.id.clone())
            .collect();
        for group in finishing {
            let detail = "Yhtye stopped while merging the group branch into the base branch. \
                          Check the base branch, then retry the merge."
                .to_string();
            self.emit(DomainEvent::GroupMergeFinished {
                group: group.clone(),
                ok: false,
                detail: detail.clone(),
            });
            self.queue_inbox(InboxItem::new(
                InboxKind::MergeResult,
                &[("group", &group), ("ok", "false")],
                detail,
            ));
        }
    }

    fn tell_orchestrator(&mut self, o: OrchestratorResume) {
        let body = if o.had_session && !o.restored {
            format!(
                "Yhtye was restarted and your previous session could not be restored, so you do \
                 not see the earlier conversation. Current state:\n{}",
                state_summary(&self.state)
            )
        } else if o.turn_was_running {
            "Yhtye was restarted while your previous turn was running, so that turn was cut off. \
             Check get_status and continue where you left off."
                .to_string()
        } else {
            return;
        };
        self.queue_inbox(InboxItem::new(InboxKind::Restarted, &[], body));
    }

    pub(super) fn resume_task(&mut self, id: &str) {
        let Some(t) = self.state.task(id) else { return };
        if t.status != TaskStatus::Interrupted {
            return;
        }
        if t.workdir.is_none() {
            return self.begin_task(id, None); // the workspace was being prepared
        }
        let Some(step) = t.current_step() else {
            return self.begin_task(id, None);
        };
        match (step.status, step.kind) {
            // Reported, but the turn end that advances the task never came.
            (StepStatus::Done, _) => {
                self.set_status(id, TaskStatus::Running);
                self.advance(id);
            }
            (StepStatus::Running, StepKind::Implement | StepKind::Review) => {
                self.resume_agent_step(id);
            }
            (StepStatus::Running, StepKind::Done) => self.start_finish(id),
            _ => self.begin_task(id, None),
        }
    }

    fn resume_agent_step(&mut self, id: &str) {
        self.set_status(id, TaskStatus::Running);
        let Some(t) = self.state.task(id) else { return };
        let index = t.current;
        let Some(role) = t.current_step().and_then(|s| s.kind.role()) else {
            return;
        };
        let effect = Effect::ResumeStep {
            agent: AgentRef {
                task: t.id.clone(),
                role,
                step: index,
            },
            group: t.group.clone(),
            prompt: resume_prompt(&t.id, index),
            fallback: format!("{}\n\n{RESTART_NOTE}", render_step_prompt(t, index)),
            workdir: t.workdir.clone(),
        };
        self.effect(effect);
    }
}

/// Groups, tasks and open helps, one per line (for an orchestrator that lost
/// its session).
fn state_summary(state: &State) -> String {
    let mut lines = Vec::new();
    for g in &state.groups {
        lines.push(format!(
            "group {} [{}]: {}",
            g.id,
            g.status.as_str(),
            g.title
        ));
        for t in state.tasks_of(&g.id) {
            let step = if t.status.is_terminal() || t.current_step().is_none() {
                String::new()
            } else {
                format!(" step {}/{}", t.current + 1, t.steps.len())
            };
            lines.push(format!(
                "  task {} [{}]{step}: {}",
                t.id,
                t.status.as_str(),
                t.title
            ));
        }
    }
    for h in state.helps.iter().filter(|h| h.is_open()) {
        lines.push(format!(
            "open help {} on {} ({}): {}",
            h.id,
            h.task,
            h.kind.as_str(),
            h.message
        ));
    }
    if lines.is_empty() {
        lines.push("(no groups yet)".into());
    }
    lines.join("\n")
}
