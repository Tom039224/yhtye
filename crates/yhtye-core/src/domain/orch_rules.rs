//! What happens when the orchestrator's turn ends (`orchestration-model.md` §3,
//! Stage 7a): a group whose tasks have all settled must not stay open because
//! the orchestrator only reported to the user and forgot `finish_group`.
//!
//! The first such turn end sends a reminder (`group_settled` with
//! `reminder=N`); after `MAX_GROUP_FINISH_NUDGES` reminders Yhtye finishes the
//! group itself (merge into the base branch, result as `merge_result`).

use super::command::{Effect, GitOp, MergeTrigger, TurnOutcome};
use super::event::DomainEvent;
use super::inbox::{InboxItem, InboxKind};
use super::machine::Tx;
use super::state::GroupStatus;
use crate::prompts::{AUTO_FINISH_SUMMARY, group_finish_reminder};

/// Reminders sent before Yhtye finishes a settled group on its own.
pub const MAX_GROUP_FINISH_NUDGES: u32 = 1;

/// A settled group: every task is terminal, or can never start.
struct Settled {
    group: String,
    /// Every task is terminal (`finish_group` would accept it).
    finishable: bool,
    nudges: u32,
}

impl Tx {
    pub(super) fn orchestrator_turn_ended(
        &mut self,
        chat: &str,
        outcome: &TurnOutcome,
        queued: bool,
    ) {
        // Only a turn the orchestrator ended itself, with nothing more to tell it:
        // a queued prompt or inbox entry gives it another turn anyway.
        if *outcome != TurnOutcome::EndTurn || queued || self.state.inbox_of(chat).next().is_some()
        {
            return;
        }
        let Some(s) = self.settled_open_group(chat) else {
            return;
        };
        if s.nudges < MAX_GROUP_FINISH_NUDGES {
            self.emit(DomainEvent::GroupFinishReminded {
                group: s.group.clone(),
            });
            let count = (s.nudges + 1).to_string();
            self.queue_inbox(
                chat,
                InboxItem::new(
                    InboxKind::GroupSettled,
                    &[("group", s.group.as_str()), ("reminder", count.as_str())],
                    group_finish_reminder(&s.group),
                ),
            );
        } else if s.finishable {
            self.auto_finish(&s.group);
        }
        // Tasks that can never start still need the orchestrator (`cancel_task`);
        // the group stays open and the UI shows why.
    }

    fn settled_open_group(&self, chat: &str) -> Option<Settled> {
        let g = self
            .state
            .groups
            .iter()
            .find(|g| g.chat == chat && g.status == GroupStatus::Active)?;
        let mut tasks = self.state.tasks_of(&g.id).peekable();
        tasks.peek()?;
        let mut finishable = true;
        for t in tasks {
            if t.status.is_terminal() {
                continue;
            }
            if !self.state.is_blocked(t) {
                return None;
            }
            finishable = false;
        }
        Some(Settled {
            group: g.id.clone(),
            finishable,
            nudges: g.finish_nudges,
        })
    }

    /// `finish_group` on the orchestrator's behalf.
    fn auto_finish(&mut self, group: &str) {
        let Some(g) = self.state.group(group) else {
            return;
        };
        let op = GitOp::MergeGroup {
            group: g.id.clone(),
            group_branch: g.group_branch.clone(),
            base_branch: g.base_branch.clone(),
            trigger: MergeTrigger::Yhtye,
        };
        self.emit(DomainEvent::GroupFinishing {
            group: group.to_string(),
            summary: AUTO_FINISH_SUMMARY.to_string(),
        });
        self.effect(Effect::Git(op));
    }
}
