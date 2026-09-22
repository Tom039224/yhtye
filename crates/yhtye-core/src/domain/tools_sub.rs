//! Sub-agent tools (`report_step_done`, `help`) and the read-only orchestrator
//! tools (`get_status`, `get_task`) (`mcp-tools.md` §3–§4).

use serde_json::{Value, json};

use super::event::DomainEvent;
use super::machine::{Tx, non_empty};
use super::state::{StepStatus, Task, TaskStatus};
use super::tools_orch::{agent_source, step_list};
use super::types::{HelpKind, Role, StepKind, StepSpec, ToolError, Verdict};
use crate::mcp::SessionBinding;
use crate::mcp::tools::{GetStatusArgs, GetTaskArgs, HelpArgs, ReportStepDoneArgs};

type Reply = Result<Value, ToolError>;

impl Tx {
    pub(super) fn report_step_done(&mut self, b: &SessionBinding, a: ReportStepDoneArgs) -> Reply {
        non_empty("result", &a.result)?;
        match (b.role, a.verdict) {
            (Role::Reviewer, None) => {
                return Err(ToolError::invalid_argument(
                    "reviewers must give a verdict: approve or needs_changes",
                ));
            }
            (Role::Implementer, Some(_)) => {
                return Err(ToolError::invalid_argument(
                    "implementers must not give a verdict; omit it",
                ));
            }
            _ => {}
        }
        let (task, index) = self.bound_running_step(b)?;
        self.emit(DomainEvent::StepCompleted {
            task: task.clone(),
            step: index,
            result: a.result.clone(),
            verdict: a.verdict,
        });
        if a.verdict == Some(Verdict::NeedsChanges) {
            self.request_changes(&task, index, &a.result);
        }
        Ok(json!({ "ok": true, "next": "end_turn" }))
    }

    /// `needs_changes`: insert implement (with the findings) + review after the
    /// review, or ask the orchestrator once the rounds are used up.
    fn request_changes(&mut self, id: &str, review: usize, findings: &str) {
        let Some(t) = self.state.task(id) else { return };
        if t.review_rounds >= self.state.config.max_review_rounds {
            let message = format!(
                "The review still asks for changes after {} automatic rounds. Latest findings:\n{findings}\n\
                 Resume to accept the work as is, modify_steps to add more work first, or cancel the task.",
                t.review_rounds
            );
            return self.raise_yhtye_help(id, HelpKind::ReviewRoundsExhausted, &message);
        }
        let original = t.instruction.as_deref().unwrap_or("(none)");
        let fix = StepSpec {
            kind: StepKind::Implement,
            instruction: Some(format!(
                "A review of your changes asked for changes. Address these findings:\n{findings}\n\n\
                 Original instruction:\n{original}"
            )),
        };
        self.emit(DomainEvent::ReviewStepsInserted {
            task: id.to_string(),
            after: review,
            steps: vec![fix, StepSpec::new(StepKind::Review)],
        });
    }

    pub(super) fn help(&mut self, b: &SessionBinding, a: HelpArgs) -> Reply {
        non_empty("message", &a.message)?;
        let task_id = bound_task_id(b)?;
        if let Some(h) = self.state.open_help(&task_id) {
            return Err(ToolError::invalid_state(format!(
                "help {} for this task is still waiting for an answer; end your turn and wait",
                h.id
            )));
        }
        let (task, _) = self.bound_running_step(b)?;
        let help_id = self
            .raise_help(&task, a.kind.into(), &a.message, agent_source(b))
            .ok_or_else(|| ToolError::internal("could not raise help"))?;
        Ok(json!({ "help_id": help_id, "next": "end_turn_and_wait" }))
    }

    /// The task and step the caller is bound to, if that step is running now.
    fn bound_running_step(&self, b: &SessionBinding) -> Result<(String, usize), ToolError> {
        let task_id = bound_task_id(b)?;
        let t = self.task(&task_id)?;
        let step = b.step.and_then(|i| t.steps.get(i));
        let running = t.status == TaskStatus::Running
            && b.step == Some(t.current)
            && step
                .is_some_and(|s| s.status == StepStatus::Running && s.kind.role() == Some(b.role));
        if !running {
            return Err(ToolError::invalid_state(format!(
                "your step is not the running step of task {} (task is {}, current step {}); end your turn",
                t.id,
                t.status.as_str(),
                t.current
            )));
        }
        Ok((task_id, t.current))
    }

    pub(super) fn get_status(&self, a: &GetStatusArgs) -> Reply {
        let group = match &a.group_id {
            Some(id) => self
                .state
                .group(id)
                .ok_or_else(|| ToolError::not_found(format!("no group {id}")))?,
            None => self
                .state
                .open_group()
                .ok_or_else(|| ToolError::not_found("there is no active group"))?,
        };
        let tasks: Vec<Value> = self.state.tasks_of(&group.id).map(task_summary).collect();
        let open_helps: Vec<Value> = self
            .state
            .helps
            .iter()
            .filter(|h| h.is_open())
            .filter(|h| self.state.task(&h.task).is_some_and(|t| t.group == group.id))
            .map(|h| json!({ "help_id": h.id, "task": h.task, "kind": h.kind, "message": h.message }))
            .collect();
        Ok(json!({ "group": group, "tasks": tasks, "open_helps": open_helps }))
    }

    pub(super) fn get_task(&self, a: &GetTaskArgs) -> Reply {
        let t = self.task(&a.task_id)?;
        let helps: Vec<Value> = self
            .state
            .helps
            .iter()
            .filter(|h| h.task == t.id && h.is_open())
            .map(|h| json!({ "help_id": h.id, "kind": h.kind, "message": h.message }))
            .collect();
        let steps: Vec<Value> = t
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                json!({ "index": i, "kind": s.kind, "status": s.status, "instruction": s.instruction,
                        "result": s.result, "verdict": s.verdict })
            })
            .collect();
        Ok(json!({
            "task_id": t.id, "group": t.group, "title": t.title, "kind": t.kind, "status": t.status,
            "depends_on": t.depends_on, "instruction": t.instruction, "current_step": t.current,
            "steps": steps, "branch": t.branch(), "worktree": t.workdir, "open_helps": helps,
        }))
    }
}

fn bound_task_id(b: &SessionBinding) -> Result<String, ToolError> {
    b.task
        .clone()
        .ok_or_else(|| ToolError::forbidden("this session is not bound to a task"))
}

fn task_summary(t: &Task) -> Value {
    let summary: Vec<String> = step_list(t)
        .iter()
        .map(|s| {
            format!(
                "{}:{}",
                s["kind"].as_str().unwrap_or("?"),
                s["status"].as_str().unwrap_or("?")
            )
        })
        .collect();
    json!({
        "task_id": t.id, "title": t.title, "kind": t.kind, "status": t.status,
        "depends_on": t.depends_on, "current_step": t.current_step().map(|s| s.kind),
        "steps_summary": summary,
    })
}
