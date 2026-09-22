//! Role system prompts (`crates/yhtye-core/prompts/*.md`) and the prompts Yhtye
//! sends to sub-agents (`orchestration-model.md` §7–§8).

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

/// Sent once when a sub-agent's turn ends without `report_step_done` or `help`.
#[must_use]
pub fn reminder_prompt(task_id: &str, step_index: usize) -> String {
    format!(
        "[yhtye:reminder] task={task_id} step={}\nYour turn ended without calling \
         report_step_done or help. If the step is finished, call report_step_done with your \
         result now. Otherwise finish the work and then call it, or call help if you are stuck. \
         Then end your turn.",
        step_index + 1
    )
}

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
        });
        assert!(text.starts_with("[yhtye:step] task=T-1 kind=code step=1/2 step_kind=implement\n"));
        assert!(text.contains("Instruction:\nAppend 'hello' to README.md\n"));
        assert!(text.contains("Results of earlier steps:\nstep 0 (implement): added\n"));
        assert!(text.contains("Note from the orchestrator:\nkeep it short\n"));
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
}
