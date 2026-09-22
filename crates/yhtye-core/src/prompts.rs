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
    format!(
        "[yhtye:step] task={} kind={} step={}/{} step_kind={}\nTask: {}\n\nInstruction:\n{}\n\n\
         When finished, call report_step_done; if stuck, call help. Then end your turn.",
        p.task_id,
        task_kind_str(p.task_kind),
        p.step_index + 1,
        p.step_count,
        step_kind_str(p.step_kind),
        p.task_title,
        p.instruction.trim(),
    )
}

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
        });
        assert!(text.starts_with("[yhtye:step] task=T-1 kind=code step=1/2 step_kind=implement\n"));
        assert!(text.contains("Instruction:\nAppend 'hello' to README.md\n"));
    }
}
