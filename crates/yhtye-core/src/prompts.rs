//! Role system prompts (`crates/yhtye-core/prompts/*.md`) and the prompts Yhtye
//! sends to sub-agents (`orchestration-model.md` §7–§8).

use crate::agents::{AgentChoice, AgentRole, AgentSettings};
use crate::domain::{Role, StepKind, TaskKind};

const ORCHESTRATOR: &str = include_str!("../prompts/orchestrator.md");
const IMPLEMENTER: &str = include_str!("../prompts/implementer.md");
const REVIEWER: &str = include_str!("../prompts/reviewer.md");

/// System prompt appended for a session of `role`.
#[must_use]
pub fn system_prompt(role: Role) -> &'static str {
    match role {
        Role::Orchestrator => ORCHESTRATOR,
        Role::Implementer => IMPLEMENTER,
        Role::Reviewer => REVIEWER,
    }
}

/// Appended to the orchestrator's system prompt: the candidate rows (harness ×
/// model × effort, with their notes) `create_task` accepts, as of the session
/// start (`core-design.md` §15.5).
#[must_use]
pub fn agent_choices_prompt(settings: &AgentSettings) -> String {
    let mut text = String::from(
        "\n\n## Agents (harness × model × effort)\n\nEach role below has candidate rows as they \
         were when this session started (`get_status` has the current ones under `agents`); the \
         note says when to use a row. Normally omit `harness` / `model` / `effort` in \
         `create_task` and the role's default (marked default) is used. To pick a row for a \
         task, pass its harness, model and effort exactly as listed (effort may be omitted when \
         only one row has that harness and model). Anything that does not match a row is \
         rejected.\n",
    );
    for (role, what, args) in [
        (
            AgentRole::Implementer,
            "code tasks",
            "harness / model / effort",
        ),
        (
            AgentRole::Investigator,
            "investigate tasks",
            "harness / model / effort",
        ),
        (
            AgentRole::Reviewer,
            "review steps",
            "review_harness / review_model / review_effort",
        ),
    ] {
        let s = settings.get(role);
        text.push_str(&format!(
            "\n- {} ({what}, `{args}`): default {}\n",
            role.as_str(),
            choice_text(&s.default),
        ));
        for c in &s.candidates {
            let note = c.note.trim();
            let mark = if c.choice() == s.default {
                " [default]"
            } else {
                ""
            };
            if note.is_empty() {
                text.push_str(&format!("  - {}{mark}\n", choice_text(&c.choice())));
            } else {
                text.push_str(&format!(
                    "  - {}{mark} — {note}\n",
                    choice_text(&c.choice())
                ));
            }
        }
    }
    text
}

/// `harness=claude-code model=haiku effort=high` (no model: the harness default;
/// no effort: it is not set).
fn choice_text(c: &AgentChoice) -> String {
    let mut text = match &c.model {
        Some(m) => format!("harness={} model={m}", c.harness),
        None => format!("harness={} (its default model)", c.harness),
    };
    if let Some(e) = &c.effort {
        text.push_str(&format!(" effort={e}"));
    }
    text
}

/// What a sub-agent needs to start a step.
#[derive(Debug, Clone)]
pub struct StepPrompt<'a> {
    pub task_id: &'a str,
    pub task_title: &'a str,
    pub task_kind: TaskKind,
    pub step_index: usize,
    pub step_count: usize,
    pub step_kind: StepKind,
    pub instruction: &'a str,
    /// Results of earlier steps (given to reviewers).
    pub context: Option<&'a str>,
    /// Note from the orchestrator (checkpoint `note`, help reply on restart).
    pub note: Option<&'a str>,
    /// For a review of a `code` task: `(task branch, group branch)` — the diff
    /// to review is the task branch (with uncommitted changes) since it forked.
    pub review_range: Option<(&'a str, &'a str)>,
}

fn task_kind_str(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::Code => "code",
        TaskKind::Investigate => "investigate",
    }
}

fn step_kind_str(kind: StepKind) -> &'static str {
    match kind {
        StepKind::Implement => "implement",
        StepKind::Review => "review",
        StepKind::Checkpoint => "checkpoint",
        StepKind::Done => "done",
    }
}

/// The prompt that starts a step: a machine-readable header, then the instruction.
#[must_use]
pub fn step_prompt(p: &StepPrompt<'_>) -> String {
    let mut text = format!(
        "[yhtye:step] task={} kind={} step={}/{} step_kind={}\nTask: {}\n\nInstruction:\n{}\n",
        p.task_id,
        task_kind_str(p.task_kind),
        p.step_index + 1,
        p.step_count,
        step_kind_str(p.step_kind),
        p.task_title,
        p.instruction.trim(),
    );
    if let Some((task_branch, group_branch)) = p.review_range {
        text.push_str(&review_range_text(task_branch, group_branch));
    }
    if let Some(context) = p.context {
        text.push_str(&format!(
            "\nResults of earlier steps:\n{}\n",
            context.trim()
        ));
    }
    if let Some(note) = p.note {
        text.push_str(&format!("\nNote from the orchestrator:\n{}\n", note.trim()));
    }
    text.push_str(
        "\nWhen finished, call report_step_done; if stuck, call help. Then end your turn.",
    );
    text
}

/// Where a reviewer finds the changes it reviews.
fn review_range_text(task_branch: &str, group_branch: &str) -> String {
    format!(
        "\nChanges to review: your working directory is the task branch `{task_branch}`, forked \
         from the group branch `{group_branch}`. Run `git status --short` and \
         `git diff $(git merge-base {group_branch} HEAD)` there: the diff includes commits and \
         uncommitted changes; read new (untracked) files listed by git status directly.\n"
    )
}

/// Appended to a restart fallback prompt: what is already in the working directory.
#[must_use]
pub fn workspace_changes_note(changes: &str) -> String {
    format!(
        "\n\nChanges already in the working directory (git status and git diff since the task \
         branch forked):\n```\n{}\n```",
        changes.trim_end()
    )
}

/// Sent once when a sub-agent's turn ends without `report_step_done` or `help`.
#[must_use]
pub fn reminder_prompt(task_id: &str, step_index: usize) -> String {
    format!(
        "[yhtye:reminder] task={task_id} step={}\nYour turn ended without calling \
         report_step_done or help. If the step is finished, call report_step_done with your \
         result now. Otherwise finish the work and then call it, or call help if you are stuck. \
         If you were waiting for a command, run it again in the foreground and wait for it to \
         finish in this turn. Then end your turn.",
        step_index + 1
    )
}

/// Body of the `group_settled` reminder sent when the orchestrator ends its turn
/// while every task of an open group has settled (Stage 7a).
#[must_use]
pub fn group_finish_reminder(group: &str) -> String {
    format!(
        "Reminder: every task of group {group} has finished, but the group is still open, so \
         its work is not merged into the user's branch yet. Call finish_group for {group} now \
         with a short summary for the user (or create_task to add more work, cancel_task tasks \
         that cannot start, or cancel_group to drop the group), then tell the user the result. \
         If you end your turn without doing so, Yhtye will finish the group itself."
    )
}

/// `finish_summary` of a group Yhtye finished after the reminder (Stage 7a).
pub const AUTO_FINISH_SUMMARY: &str =
    "Finished by Yhtye: the orchestrator ended its turn without finish_group after a reminder.";

/// Sent to a sub-agent session restored with `session/load` after a restart.
#[must_use]
pub fn resume_prompt(task_id: &str, step_index: usize) -> String {
    format!(
        "[yhtye:resume] task={task_id} step={}\nYhtye was restarted and your previous turn was \
         interrupted. Continue the current step where you left off (check the working directory \
         for work you already did). When finished, call report_step_done; if stuck, call help. \
         Then end your turn.",
        step_index + 1
    )
}

/// Appended to the step prompt when a restarted step gets a new session because
/// its previous session could not be restored.
pub const RESTART_NOTE: &str = "Note from Yhtye: Yhtye was restarted and the session that was \
    working on this step could not be restored. Part of the work may already be done in the \
    working directory; check it (for example with git status and git diff) before continuing.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_has_a_prompt_naming_its_tools() {
        assert!(system_prompt(Role::Orchestrator).contains("create_task"));
        for role in [Role::Implementer, Role::Reviewer] {
            let p = system_prompt(role);
            assert!(
                p.contains("report_step_done") && p.contains("help"),
                "{role}"
            );
        }
        assert!(system_prompt(Role::Reviewer).contains("verdict"));
    }

    #[test]
    fn step_prompt_has_header_and_instruction() {
        let text = step_prompt(&StepPrompt {
            task_id: "T-1",
            task_title: "Add a line",
            task_kind: TaskKind::Code,
            step_index: 0,
            step_count: 2,
            step_kind: StepKind::Implement,
            instruction: "  Append 'hello' to README.md\n",
            context: Some("step 0 (implement): added"),
            note: Some("keep it short"),
            review_range: None,
        });
        assert!(text.starts_with("[yhtye:step] task=T-1 kind=code step=1/2 step_kind=implement\n"));
        assert!(text.contains("Instruction:\nAppend 'hello' to README.md\n"));
        assert!(text.contains("Results of earlier steps:\nstep 0 (implement): added\n"));
        assert!(text.contains("Note from the orchestrator:\nkeep it short\n"));
    }

    #[test]
    fn review_prompt_names_the_diff_range() {
        let text = step_prompt(&StepPrompt {
            task_id: "T-2",
            task_title: "Review",
            task_kind: TaskKind::Code,
            step_index: 1,
            step_count: 3,
            step_kind: StepKind::Review,
            instruction: "check it",
            context: None,
            note: None,
            review_range: Some(("yhtye/G-1-T-2", "yhtye/G-1")),
        });
        assert!(text.contains("task branch `yhtye/G-1-T-2`"));
        assert!(text.contains("git diff $(git merge-base yhtye/G-1 HEAD)"));
    }

    #[test]
    fn resume_prompt_has_header() {
        let text = resume_prompt("T-3", 1);
        assert!(text.starts_with("[yhtye:resume] task=T-3 step=2\n"));
        assert!(text.contains("report_step_done"));
    }

    #[test]
    fn reminder_names_both_tools() {
        let text = reminder_prompt("T-2", 0);
        assert!(text.starts_with("[yhtye:reminder] task=T-2 step=1\n"));
        assert!(text.contains("report_step_done") && text.contains("help"));
    }

    #[test]
    fn agent_choices_list_each_row_with_its_note() {
        use crate::agents::{AgentSettingsLayer, Candidate, RoleSettings, effective};
        let builtin = AgentChoice::new("claude-code", Some("haiku"));
        let mut layer = AgentSettingsLayer::default();
        let deep = AgentChoice::new("claude-code", Some("sonnet")).with_effort("high");
        layer.set(
            AgentRole::Reviewer,
            Some(RoleSettings {
                candidates: vec![
                    Candidate::from(builtin.clone()).with_note("cheap first pass"),
                    Candidate::from(deep.clone()).with_note("for risky changes"),
                    AgentChoice::new("opencode", None).into(),
                ],
                default: deep,
            }),
        );
        let text = agent_choices_prompt(&effective(&builtin, &layer, None));
        assert!(text.contains(
            "- implementer (code tasks, `harness / model / effort`): default harness=claude-code model=haiku\n  \
             - harness=claude-code model=haiku [default]\n"
        ));
        assert!(text.contains("- investigator (investigate tasks"));
        assert!(text.contains(
            "- reviewer (review steps, `review_harness / review_model / review_effort`): default \
             harness=claude-code model=sonnet effort=high\n"
        ));
        assert!(text.contains("  - harness=claude-code model=haiku — cheap first pass\n"));
        assert!(text.contains(
            "  - harness=claude-code model=sonnet effort=high [default] — for risky changes\n"
        ));
        assert!(text.contains("  - harness=opencode (its default model)\n"));
        assert!(
            !text.contains("orchestrator ("),
            "the orchestrator's own role is not offered"
        );
    }
}
