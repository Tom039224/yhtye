//! Tests of the Stage 2 in-memory board.

use serde_json::{Value, json};

use super::*;
use crate::domain::ErrorCode;
use crate::mcp::tools::{ToolName, parse_call};

fn orch() -> SessionBinding {
    SessionBinding::orchestrator("orchestrator", "P-1")
}

fn sub(role: Role, task: &str, step: usize) -> SessionBinding {
    SessionBinding {
        session: format!("{task}/{step}"),
        role,
        project: "P-1".into(),
        group: Some("G-1".into()),
        task: Some(task.into()),
        step: Some(step),
    }
}

fn call(board: &mut Board, b: &SessionBinding, tool: ToolName, args: Value) -> Outcome {
    let call = parse_call(tool, args.as_object().cloned()).expect("valid args");
    board.apply(b, call)
}

fn ok(board: &mut Board, b: &SessionBinding, tool: ToolName, args: Value) -> (Value, Vec<Effect>) {
    call(board, b, tool, args).unwrap_or_else(|e| panic!("{tool:?} failed: {e}"))
}

fn err(board: &mut Board, b: &SessionBinding, tool: ToolName, args: Value) -> ErrorCode {
    call(board, b, tool, args).expect_err("must fail").code
}

fn with_group() -> Board {
    let mut board = Board::new("main");
    ok(
        &mut board,
        &orch(),
        ToolName::CreateGroup,
        json!({"title": "g"}),
    );
    board
}

fn create_task(board: &mut Board, extra: Value) -> (Value, Vec<Effect>) {
    let mut args = json!({"group_id": "G-1", "title": "t", "kind": "code",
                          "steps": [{"kind": "implement"}], "instruction": "do it"});
    if let (Some(a), Some(e)) = (args.as_object_mut(), extra.as_object()) {
        a.extend(e.clone());
    }
    ok(board, &orch(), ToolName::CreateTask, args)
}

fn started(effects: &[Effect]) -> Vec<(String, usize, Role)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::StartStep(s) => Some((s.task.clone(), s.step, s.role)),
            _ => None,
        })
        .collect()
}

fn wakes(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Wake(item) => Some(item.render()),
            _ => None,
        })
        .collect()
}

#[test]
fn create_group_returns_branches_and_allows_one_active_group() {
    let mut board = Board::new("main");
    let (reply, _) = ok(
        &mut board,
        &orch(),
        ToolName::CreateGroup,
        json!({"title": "g"}),
    );
    assert_eq!(
        reply,
        json!({"group_id": "G-1", "group_branch": "yhtye/G-1", "base_branch": "main"})
    );
    let code = err(
        &mut board,
        &orch(),
        ToolName::CreateGroup,
        json!({"title": "h"}),
    );
    assert_eq!(code, ErrorCode::Conflict);
    let code = err(
        &mut Board::new("main"),
        &orch(),
        ToolName::CreateGroup,
        json!({"title": " "}),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

#[test]
fn create_task_starts_first_step_and_normalizes_steps() {
    let mut board = with_group();
    let (reply, effects) = create_task(&mut board, json!({}));
    assert_eq!(reply["task_id"], "T-1");
    assert_eq!(reply["status"], "running");
    assert_eq!(
        reply["steps"],
        json!([{"kind": "implement"}, {"kind": "done"}])
    );
    assert_eq!(
        started(&effects),
        vec![("T-1".into(), 0, Role::Implementer)]
    );

    let code = err(
        &mut board,
        &orch(),
        ToolName::CreateTask,
        json!({"group_id": "G-9", "title": "t", "kind": "code", "steps": [{"kind": "implement"}]}),
    );
    assert_eq!(code, ErrorCode::NotFound);
    let code = err(
        &mut board,
        &orch(),
        ToolName::CreateTask,
        json!({"group_id": "G-1", "title": "t", "kind": "investigate", "steps": [{"kind": "review"}]}),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

#[test]
fn report_then_turn_end_finishes_task_and_settles_group() {
    let mut board = with_group();
    create_task(&mut board, json!({}));
    let b = sub(Role::Implementer, "T-1", 0);
    let (reply, effects) = ok(
        &mut board,
        &b,
        ToolName::ReportStepDone,
        json!({"result": "added"}),
    );
    assert_eq!(reply, json!({"ok": true, "next": "end_turn"}));
    assert!(effects.is_empty(), "next step waits for the turn to end");
    assert_eq!(
        err(
            &mut board,
            &b,
            ToolName::ReportStepDone,
            json!({"result": "again"})
        ),
        ErrorCode::InvalidState
    );

    let effects = board.on_turn_ended("T-1");
    assert!(effects.contains(&Effect::TaskFinished { task: "T-1".into() }));
    assert_eq!(
        wakes(&effects),
        vec!["[yhtye:group_settled] group=G-1\nT-1 done: added"]
    );
    assert_eq!(board.task("T-1").map(|t| t.status), Some(TaskStatus::Done));
    assert!(board.on_turn_ended("T-1").is_empty());

    let (reply, _) = ok(
        &mut board,
        &orch(),
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "s"}),
    );
    assert_eq!(reply["status"], "done");
}

#[test]
fn verdict_rules_depend_on_role() {
    let mut board = with_group();
    create_task(
        &mut board,
        json!({"steps": [{"kind": "implement"}, {"kind": "review"}]}),
    );
    let imp = sub(Role::Implementer, "T-1", 0);
    let code = err(
        &mut board,
        &imp,
        ToolName::ReportStepDone,
        json!({"result": "x", "verdict": "approve"}),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
    ok(
        &mut board,
        &imp,
        ToolName::ReportStepDone,
        json!({"result": "x"}),
    );
    let effects = board.on_turn_ended("T-1");
    assert_eq!(started(&effects), vec![("T-1".into(), 1, Role::Reviewer)]);

    let rev = sub(Role::Reviewer, "T-1", 1);
    assert_eq!(
        err(
            &mut board,
            &rev,
            ToolName::ReportStepDone,
            json!({"result": "ok"})
        ),
        ErrorCode::InvalidArgument
    );
    ok(
        &mut board,
        &rev,
        ToolName::ReportStepDone,
        json!({"result": "ok", "verdict": "approve"}),
    );
    assert!(
        board
            .on_turn_ended("T-1")
            .contains(&Effect::TaskFinished { task: "T-1".into() })
    );
}

#[test]
fn wrong_step_and_unbound_sessions_are_rejected() {
    let mut board = with_group();
    create_task(&mut board, json!({}));
    let stale = sub(Role::Implementer, "T-1", 3);
    assert_eq!(
        err(
            &mut board,
            &stale,
            ToolName::ReportStepDone,
            json!({"result": "x"})
        ),
        ErrorCode::InvalidState
    );
    let code = err(
        &mut board,
        &orch(),
        ToolName::ReportStepDone,
        json!({"result": "x"}),
    );
    assert_eq!(code, ErrorCode::Forbidden);
    let other = sub(Role::Implementer, "T-7", 0);
    assert_eq!(
        err(
            &mut board,
            &other,
            ToolName::Help,
            json!({"kind": "blocked", "message": "m"})
        ),
        ErrorCode::NotFound
    );
}

#[test]
fn help_wakes_orchestrator_and_answer_resumes_agent() {
    let mut board = with_group();
    create_task(&mut board, json!({}));
    let b = sub(Role::Implementer, "T-1", 0);
    let (reply, effects) = ok(
        &mut board,
        &b,
        ToolName::Help,
        json!({"kind": "question", "message": "which file?"}),
    );
    assert_eq!(
        reply,
        json!({"help_id": "H-1", "next": "end_turn_and_wait"})
    );
    assert_eq!(
        wakes(&effects),
        vec!["[yhtye:help_raised] help_id=H-1 task=T-1 kind=question\nwhich file?"]
    );
    assert_eq!(
        board.task("T-1").map(|t| t.status),
        Some(TaskStatus::Handling)
    );
    assert_eq!(
        err(
            &mut board,
            &b,
            ToolName::Help,
            json!({"kind": "blocked", "message": "m"})
        ),
        ErrorCode::InvalidState
    );

    let code = err(
        &mut board,
        &orch(),
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "resume"}),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
    let (reply, effects) = ok(
        &mut board,
        &orch(),
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "resume", "reply": "README.md"}),
    );
    assert_eq!(reply["task_status"], "running");
    assert_eq!(
        effects,
        vec![Effect::PromptTask {
            task: "T-1".into(),
            text: "[yhtye:help_answer] help_id=H-1\nREADME.md".into(),
        }]
    );
    let code = err(
        &mut board,
        &orch(),
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "cancel_task"}),
    );
    assert_eq!(code, ErrorCode::InvalidState);
    ok(
        &mut board,
        &b,
        ToolName::ReportStepDone,
        json!({"result": "done"}),
    );
}

#[test]
fn dependency_without_instruction_asks_for_it() {
    let mut board = with_group();
    create_task(&mut board, json!({}));
    let (reply, effects) = create_task(
        &mut board,
        json!({"depends_on": ["T-1"], "instruction": ""}),
    );
    assert_eq!(reply["status"], "pending");
    assert!(effects.is_empty());
    let code = err(
        &mut board,
        &orch(),
        ToolName::CreateTask,
        json!({"group_id": "G-1", "title": "t", "kind": "code", "steps": [{"kind": "implement"}], "depends_on": ["T-9"]}),
    );
    assert_eq!(code, ErrorCode::NotFound);

    ok(
        &mut board,
        &sub(Role::Implementer, "T-1", 0),
        ToolName::ReportStepDone,
        json!({"result": "base ready"}),
    );
    let effects = board.on_turn_ended("T-1");
    assert_eq!(
        wakes(&effects),
        vec!["[yhtye:instruction_needed] task=T-2\nT-1: base ready"]
    );
    assert_eq!(
        board.task("T-2").map(|t| t.status),
        Some(TaskStatus::AwaitingInstruction)
    );

    let (reply, effects) = ok(
        &mut board,
        &orch(),
        ToolName::SetInstruction,
        json!({"task_id": "T-2", "instruction": "go"}),
    );
    assert_eq!(reply["status"], "running");
    assert_eq!(
        started(&effects),
        vec![("T-2".into(), 0, Role::Implementer)]
    );
    let code = err(
        &mut board,
        &orch(),
        ToolName::SetInstruction,
        json!({"task_id": "T-2", "instruction": "again"}),
    );
    assert_eq!(code, ErrorCode::InvalidState);
}

#[test]
fn checkpoint_waits_for_orchestrator() {
    let mut board = with_group();
    create_task(
        &mut board,
        json!({"steps": [{"kind": "implement"}, {"kind": "checkpoint"}, {"kind": "implement"}]}),
    );
    ok(
        &mut board,
        &sub(Role::Implementer, "T-1", 0),
        ToolName::ReportStepDone,
        json!({"result": "half"}),
    );
    let effects = board.on_turn_ended("T-1");
    assert_eq!(
        wakes(&effects),
        vec!["[yhtye:checkpoint_reached] task=T-1 step=1\nstep 0 (Implement): half"]
    );
    let code = err(
        &mut board,
        &orch(),
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "s"}),
    );
    assert_eq!(code, ErrorCode::InvalidState);

    let (_, effects) = ok(
        &mut board,
        &orch(),
        ToolName::ResolveCheckpoint,
        json!({"task_id": "T-1", "decision": "continue"}),
    );
    assert_eq!(
        started(&effects),
        vec![("T-1".into(), 2, Role::Implementer)]
    );
    let code = err(
        &mut board,
        &orch(),
        ToolName::ResolveCheckpoint,
        json!({"task_id": "T-1", "decision": "abort"}),
    );
    assert_eq!(code, ErrorCode::InvalidState);
}

#[test]
fn cancel_task_settles_group_including_blocked_dependents() {
    let mut board = with_group();
    create_task(&mut board, json!({}));
    create_task(&mut board, json!({"depends_on": ["T-1"]}));
    let (reply, effects) = ok(
        &mut board,
        &orch(),
        ToolName::CancelTask,
        json!({"task_id": "T-1", "reason": "r"}),
    );
    assert_eq!(reply["status"], "cancelled");
    assert_eq!(
        wakes(&effects),
        vec![
            "[yhtye:group_settled] group=G-1\nT-1 cancelled: (no result)\nT-2 cannot start (a dependency was cancelled)"
        ]
    );
    let code = err(
        &mut board,
        &orch(),
        ToolName::CancelTask,
        json!({"task_id": "T-1", "reason": "r"}),
    );
    assert_eq!(code, ErrorCode::InvalidState);
}
