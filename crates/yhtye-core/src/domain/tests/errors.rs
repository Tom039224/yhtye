//! Every tool error of `mcp-tools.md` §3–§4 (and that failed calls change nothing).

use serde_json::json;

use super::*;
use ErrorCode::*;

fn with_task() -> Sim {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    sim
}

#[test]
fn create_group_errors() {
    let mut sim = Sim::new();
    assert_eq!(
        sim.orch_err(ToolName::CreateGroup, json!({"title": " "})),
        InvalidArgument
    );
    let detached = DomainCommand::CreateGroup {
        args: crate::mcp::tools::CreateGroupArgs {
            title: "g".into(),
            summary: None,
        },
        base_branch: None,
    };
    assert_eq!(sim.run(detached).expect_err("detached").code, InvalidState);
    sim.group();
    assert_eq!(
        sim.orch_err(ToolName::CreateGroup, json!({"title": "again"})),
        Conflict
    );
}

#[test]
fn create_task_errors() {
    let mut sim = with_task();
    let base =
        json!({"group_id": "G-1", "title": "t", "kind": "code", "steps": [{"kind": "implement"}]});
    let with = |extra: serde_json::Value| {
        let mut a = base.clone();
        if let (Some(a), Some(e)) = (a.as_object_mut(), extra.as_object()) {
            a.extend(e.clone());
        }
        a
    };
    let cases = [
        (with(json!({"group_id": "G-9"})), NotFound),
        (with(json!({"depends_on": ["T-9"]})), NotFound),
        (with(json!({"title": ""})), InvalidArgument),
        (with(json!({"steps": []})), InvalidArgument),
        (
            with(json!({"steps": [{"kind": "done"}, {"kind": "implement"}]})),
            InvalidArgument,
        ),
        (
            with(json!({"kind": "investigate", "steps": [{"kind": "review"}]})),
            InvalidArgument,
        ),
    ];
    for (args, code) in cases {
        assert_eq!(
            sim.orch_err(ToolName::CreateTask, args.clone()),
            code,
            "{args}"
        );
    }
    sim.orch_ok(
        ToolName::CancelTask,
        json!({"task_id": "T-1", "reason": "x"}),
    );
    assert_eq!(
        sim.orch_err(ToolName::CreateTask, with(json!({"depends_on": ["T-1"]}))),
        InvalidState,
        "depending on a cancelled task could never start"
    );
    sim.orch_ok(
        ToolName::CancelGroup,
        json!({"group_id": "G-1", "reason": "x"}),
    );
    assert_eq!(sim.orch_err(ToolName::CreateTask, base), InvalidState);
}

#[test]
fn set_instruction_errors() {
    let mut sim = with_task();
    let set = |task: &str, instr: &str| json!({"task_id": task, "instruction": instr});
    assert_eq!(
        sim.orch_err(ToolName::SetInstruction, set("T-9", "x")),
        NotFound
    );
    assert_eq!(
        sim.orch_err(ToolName::SetInstruction, set("T-1", " ")),
        InvalidArgument
    );
    assert_eq!(
        sim.orch_err(ToolName::SetInstruction, set("T-1", "x")),
        InvalidState
    );
}

#[test]
fn modify_steps_errors() {
    let mut sim = with_task();
    let args = |steps: serde_json::Value| json!({"task_id": "T-1", "steps": steps});
    assert_eq!(
        sim.orch_err(
            ToolName::ModifySteps,
            json!({"task_id": "T-9", "steps": []})
        ),
        NotFound
    );
    assert_eq!(
        sim.orch_err(
            ToolName::ModifySteps,
            args(json!([{"kind": "done"}, {"kind": "review"}]))
        ),
        InvalidArgument
    );
    sim.report("T-1", Role::Implementer, None);
    assert_eq!(
        sim.orch_err(ToolName::ModifySteps, args(json!([]))),
        InvalidState
    );
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({"kind": "investigate"}));
    assert_eq!(
        sim.orch_err(ToolName::ModifySteps, args(json!([{"kind": "review"}]))),
        InvalidArgument
    );
}

#[test]
fn resolve_checkpoint_errors() {
    let mut sim = with_task();
    let args = |task: &str| json!({"task_id": task, "decision": "continue"});
    assert_eq!(
        sim.orch_err(ToolName::ResolveCheckpoint, args("T-9")),
        NotFound
    );
    assert_eq!(
        sim.orch_err(ToolName::ResolveCheckpoint, args("T-1")),
        InvalidState
    );
}

#[test]
fn answer_help_errors() {
    let mut sim = with_task();
    let answer = |action: &str| json!({"help_id": "H-1", "action": action});
    assert_eq!(
        sim.orch_err(ToolName::AnswerHelp, answer("resume")),
        NotFound
    );
    sim.sub_ok(
        sub("T-1", Role::Implementer, 0),
        ToolName::Help,
        json!({"kind": "question", "message": "?"}),
    );
    assert_eq!(
        sim.orch_err(ToolName::AnswerHelp, answer("resume")),
        InvalidArgument,
        "an agent's help needs a reply to resume"
    );
    sim.orch_ok(
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "resume", "reply": "a"}),
    );
    assert_eq!(
        sim.orch_err(ToolName::AnswerHelp, answer("cancel_task")),
        InvalidState
    );
}

#[test]
fn cancel_task_errors() {
    let mut sim = with_task();
    let args = |task: &str| json!({"task_id": task, "reason": "r"});
    assert_eq!(sim.orch_err(ToolName::CancelTask, args("T-9")), NotFound);
    sim.orch_ok(ToolName::CancelTask, args("T-1"));
    assert_eq!(
        sim.orch_err(ToolName::CancelTask, args("T-1")),
        InvalidState
    );
}

#[test]
fn group_tool_errors() {
    let mut sim = with_task();
    let finish = |g: &str| json!({"group_id": g, "summary": "s"});
    let cancel = |g: &str| json!({"group_id": g, "reason": "r"});
    assert_eq!(sim.orch_err(ToolName::FinishGroup, finish("G-9")), NotFound);
    assert_eq!(sim.orch_err(ToolName::CancelGroup, cancel("G-9")), NotFound);
    assert_eq!(
        sim.orch_err(ToolName::GetStatus, json!({"group_id": "G-9"})),
        NotFound
    );
    assert_eq!(
        sim.orch_err(ToolName::GetTask, json!({"task_id": "T-9"})),
        NotFound
    );
    let code = sim.orch_err(ToolName::FinishGroup, finish("G-1"));
    assert_eq!(code, InvalidState, "T-1 is still running");
    sim.report("T-1", Role::Implementer, None);
    sim.orch_ok(ToolName::FinishGroup, finish("G-1"));
    assert_eq!(
        sim.orch_err(ToolName::FinishGroup, finish("G-1")),
        InvalidState
    );
    assert_eq!(
        sim.orch_err(ToolName::CancelGroup, cancel("G-1")),
        InvalidState
    );
    assert_eq!(
        sim.orch_err(ToolName::GetStatus, json!({})),
        NotFound,
        "no active group"
    );
}

#[test]
fn report_step_done_errors() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({"steps": [{"kind": "implement"}, {"kind": "review"}]}));
    let imp = sub("T-1", Role::Implementer, 0);
    let rev = sub("T-1", Role::Reviewer, 1);
    let report = |r: &str| json!({"result": r});
    assert_eq!(
        sim.sub_err(imp.clone(), ToolName::ReportStepDone, report(" ")),
        InvalidArgument
    );
    assert_eq!(
        sim.sub_err(
            imp.clone(),
            ToolName::ReportStepDone,
            json!({"result": "x", "verdict": "approve"})
        ),
        InvalidArgument
    );
    assert_eq!(
        sim.sub_err(rev.clone(), ToolName::ReportStepDone, report("x")),
        InvalidArgument,
        "reviewer without verdict"
    );
    assert_eq!(
        sim.sub_err(
            rev,
            ToolName::ReportStepDone,
            json!({"result": "x", "verdict": "approve"})
        ),
        InvalidState,
        "the review step has not started"
    );
    sim.sub_ok(imp.clone(), ToolName::ReportStepDone, report("x"));
    assert_eq!(
        sim.sub_err(imp, ToolName::ReportStepDone, report("again")),
        InvalidState,
        "already reported"
    );
    let mut unbound = sub("T-1", Role::Implementer, 0);
    unbound.task = None;
    assert_eq!(
        sim.sub_err(unbound, ToolName::ReportStepDone, report("x")),
        Forbidden
    );
}

#[test]
fn help_errors() {
    let mut sim = with_task();
    let imp = sub("T-1", Role::Implementer, 0);
    let help = json!({"kind": "blocked", "message": "m"});
    assert_eq!(
        sim.sub_err(
            imp.clone(),
            ToolName::Help,
            json!({"kind": "blocked", "message": ""})
        ),
        InvalidArgument
    );
    sim.sub_ok(imp.clone(), ToolName::Help, help.clone());
    assert_eq!(sim.sub_err(imp, ToolName::Help, help), InvalidState);
}
