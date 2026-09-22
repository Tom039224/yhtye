//! Step list rules (`orchestration-model.md` §2.3, `mcp-tools.md` `create_task`).

use super::types::{StepKind, StepSpec, TaskKind, ToolError};

/// Validates the steps of a new task and appends the final `done` if missing.
///
/// Rules: at least one step; `done` only as the last step; no `review` in an
/// `investigate` task (it has no diff to review).
pub fn normalize_steps(kind: TaskKind, steps: &[StepSpec]) -> Result<Vec<StepSpec>, ToolError> {
    if steps.is_empty() {
        return Err(ToolError::invalid_argument(
            "steps must contain at least one step, e.g. [{\"kind\":\"implement\"}]",
        ));
    }
    let last = steps.len() - 1;
    if let Some(i) = steps[..last].iter().position(|s| s.kind == StepKind::Done) {
        return Err(ToolError::invalid_argument(format!(
            "steps[{i}] is `done`, but `done` may only be the last step (it is appended automatically)"
        )));
    }
    if kind == TaskKind::Investigate && steps.iter().any(|s| s.kind == StepKind::Review) {
        return Err(ToolError::invalid_argument(
            "an `investigate` task cannot have a `review` step (it produces no diff); use `implement` only",
        ));
    }
    let mut out = steps.to_vec();
    if steps[last].kind != StepKind::Done {
        out.push(StepSpec::new(StepKind::Done));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ErrorCode;

    fn specs(kinds: &[StepKind]) -> Vec<StepSpec> {
        kinds.iter().copied().map(StepSpec::new).collect()
    }

    fn kinds(steps: &[StepSpec]) -> Vec<StepKind> {
        steps.iter().map(|s| s.kind).collect()
    }

    #[test]
    fn appends_done_when_missing() {
        use StepKind::*;
        let out = normalize_steps(TaskKind::Code, &specs(&[Implement, Review])).expect("valid");
        assert_eq!(kinds(&out), vec![Implement, Review, Done]);
    }

    #[test]
    fn keeps_trailing_done() {
        use StepKind::*;
        let out = normalize_steps(TaskKind::Code, &specs(&[Implement, Done])).expect("valid");
        assert_eq!(kinds(&out), vec![Implement, Done]);
    }

    #[test]
    fn rejects_invalid_step_lists() {
        use StepKind::*;
        let cases: &[(TaskKind, &[StepKind])] = &[
            (TaskKind::Code, &[]),
            (TaskKind::Code, &[Done, Implement]),
            (TaskKind::Code, &[Implement, Done, Done]),
            (TaskKind::Investigate, &[Implement, Review]),
        ];
        for (kind, steps) in cases {
            let err = normalize_steps(*kind, &specs(steps)).expect_err("must be rejected");
            assert_eq!(err.code, ErrorCode::InvalidArgument, "{steps:?}");
        }
    }
}
