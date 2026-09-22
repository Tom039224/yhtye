//! What happens when an agent's turn ends or its process dies
//! (`orchestration-model.md` §7): advancing steps, reminders, and the help kinds
//! `protocol_violation`, `agent_stopped` and `agent_crashed`.

use super::command::{AgentRef, Effect, TurnOutcome};
use super::event::DomainEvent;
use super::flow::runs_current_step;
use super::machine::Tx;
use super::state::{HelpSource, StepStatus, TaskStatus};
use super::types::{HelpKind, Role};
use crate::prompts::reminder_prompt;

/// Reminders sent before a silent turn end becomes `protocol_violation`.
pub const MAX_NUDGES: u32 = 1;

impl Tx {
    pub(super) fn turn_ended(&mut self, agent: &AgentRef, outcome: &TurnOutcome, queued: bool) {
        let Some(t) = self.state.task(&agent.task) else {
            return;
        };
        if t.status.is_terminal() || !runs_current_step(t, agent) {
            return; // a stale session (older step or finished task)
        }
        let (status, step_status) = (t.status, t.current_step().map(|s| s.status));
        match step_status {
            Some(StepStatus::Done) => {
                if agent.role == Role::Reviewer {
                    self.effect(Effect::StopAgent {
                        agent: agent.clone(),
                    });
                }
                if status == TaskStatus::Running {
                    self.advance(&agent.task);
                }
            }
            // With a prompt queued (a help answer that arrived mid-turn) the
            // step goes on in the next turn; judge that turn instead.
            Some(StepStatus::Running) if status == TaskStatus::Running && !queued => {
                self.unreported_turn(agent, outcome);
            }
            _ => {} // waiting for a help answer
        }
    }

    /// The turn ended without `report_step_done` or `help`.
    fn unreported_turn(&mut self, agent: &AgentRef, outcome: &TurnOutcome) {
        match outcome {
            TurnOutcome::EndTurn => self.nudge_or_violation(agent),
            // Cancelled: Yhtye cancelled it while stopping the task.
            // Closed: the process is gone; `AgentExited` follows.
            TurnOutcome::Cancelled | TurnOutcome::Closed => {}
            TurnOutcome::Stopped(reason) => self.raise_yhtye_help(
                &agent.task,
                HelpKind::AgentStopped,
                &format!(
                    "The {} agent stopped its turn ({reason}) before finishing the step.",
                    agent.role
                ),
            ),
            TurnOutcome::Error(e) => self.raise_yhtye_help(
                &agent.task,
                HelpKind::AgentStopped,
                &format!("The {} agent's turn failed: {e}", agent.role),
            ),
        }
    }

    fn nudge_or_violation(&mut self, agent: &AgentRef) {
        let nudges = self
            .state
            .task(&agent.task)
            .and_then(|t| t.current_step())
            .map_or(0, |s| s.nudges);
        if nudges < MAX_NUDGES {
            self.emit(DomainEvent::NudgeSent {
                task: agent.task.clone(),
                step: agent.step,
            });
            self.effect(Effect::PromptAgent {
                agent: agent.clone(),
                text: reminder_prompt(&agent.task, agent.step),
            });
            return;
        }
        let message = format!(
            "The {} agent ended its turn {} times without calling report_step_done or help, \
             even after a reminder. Resume to restart the step (your reply is passed as a note), \
             or cancel the task.",
            agent.role,
            nudges + 1
        );
        self.raise_yhtye_help(&agent.task, HelpKind::ProtocolViolation, &message);
    }

    pub(super) fn agent_exited(&mut self, agent: &AgentRef, detail: &str) {
        let Some(t) = self.state.task(&agent.task) else {
            return;
        };
        if t.status.is_terminal() {
            return;
        }
        // An agent waiting for a help answer died: resuming must restart the step.
        let waiting_help = self
            .state
            .open_help(&agent.task)
            .filter(|h| h.source == HelpSource::Agent { role: agent.role } && h.step == agent.step);
        if let Some(help) = waiting_help.map(|h| h.id.clone()) {
            return self.emit(DomainEvent::HelpAgentLost { help });
        }
        let running = t.status == TaskStatus::Running
            && runs_current_step(t, agent)
            && t.current_step()
                .is_some_and(|s| s.status == StepStatus::Running);
        if running {
            let message = format!(
                "The {} agent's process exited unexpectedly ({detail}). Resume to restart the step \
                 in a new session (your reply is passed as a note), or cancel the task.",
                agent.role
            );
            self.raise_yhtye_help(&agent.task, HelpKind::AgentCrashed, &message);
        }
    }

    pub(super) fn agent_start_failed(&mut self, agent: &AgentRef, error: &str) {
        let Some(t) = self.state.task(&agent.task) else {
            return;
        };
        let running = t.status == TaskStatus::Running && runs_current_step(t, agent);
        if running {
            let message = format!(
                "The {} agent could not be started: {error}. Resume to try again, or cancel the task.",
                agent.role
            );
            self.raise_yhtye_help(&agent.task, HelpKind::AgentCrashed, &message);
        }
    }
}
