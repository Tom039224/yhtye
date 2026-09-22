//! Transition table: every task status × every command that targets a task.
//! Each cell is the resulting status or the error code.

use serde_json::{Value, json};

use super::*;
use ErrorCode::InvalidState;
use TaskStatus::*;

#[derive(Debug, Clone, Copy)]
enum Fixture {
    Pending,
    Awaiting,
    Running,
    Reported,
    Checkpoint,
    HandlingAgent,
    HandlingYhtye,
    Merging,
    Done,
    Cancelled,
}

const ALL: [Fixture; 10] = [
    Fixture::Pending,
    Fixture::Awaiting,
    Fixture::Running,
    Fixture::Reported,
    Fixture::Checkpoint,
    Fixture::HandlingAgent,
    Fixture::HandlingYhtye,
    Fixture::Merging,
    Fixture::Done,
    Fixture::Cancelled,
];

/// A state where task `T-2` is in the fixture's status.
fn build(f: Fixture) -> Sim {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({})); // T-1: running, a dependency for Pending
    let steps = json!([{"kind": "implement"}]);
    match f {
        Fixture::Pending => {
            sim.task(json!({"depends_on": ["T-1"]}));
        }
        Fixture::Awaiting => {
            sim.task(json!({"instruction": null}));
        }
        Fixture::Checkpoint => {
            sim.task(json!({"steps": [{"kind": "checkpoint"}, {"kind": "implement"}]}));
        }
        _ => {
            sim.task(json!({"steps": steps}));
        }
    }
    let me = sub("T-2", Role::Implementer, 0);
    let report = json!({"result": "r"});
    match f {
        Fixture::Reported => {
            sim.sub_ok(me, ToolName::ReportStepDone, report);
        }
        Fixture::HandlingAgent => {
            sim.sub_ok(
                me,
                ToolName::Help,
                json!({"kind": "blocked", "message": "m"}),
            );
        }
        Fixture::HandlingYhtye => {
            sim.run(DomainCommand::AgentExited {
                agent: agent("T-2", Role::Implementer, 0),
                detail: "x".into(),
            })
            .expect("ok");
        }
        Fixture::Merging => {
            sim.sub_ok(me, ToolName::ReportStepDone, report);
            // A single step: the merge effect is left unanswered.
            sim.step(DomainCommand::TurnEnded {
                agent: agent("T-2", Role::Implementer, 0),
                outcome: TurnOutcome::EndTurn,
                prompt_queued: false,
            })
            .expect("ok");
        }
        Fixture::Done => {
            sim.report("T-2", Role::Implementer, None);
        }
        Fixture::Cancelled => {
            sim.orch_ok(
                ToolName::CancelTask,
                json!({"task_id": "T-2", "reason": "x"}),
            );
        }
        _ => {}
    }
    sim
}

fn fixture_status(f: Fixture) -> TaskStatus {
    match f {
        Fixture::Pending => Pending,
        Fixture::Awaiting => AwaitingInstruction,
        Fixture::Running | Fixture::Reported => Running,
        Fixture::Checkpoint => Checkpoint,
        Fixture::HandlingAgent | Fixture::HandlingYhtye => Handling,
        Fixture::Merging => Merging,
        Fixture::Done => Done,
        Fixture::Cancelled => Cancelled,
    }
}

type Outcome = Result<TaskStatus, ErrorCode>;

fn apply(sim: &mut Sim, b: SessionBinding, tool: ToolName, args: Value) -> Outcome {
    match sim.tool(b, tool, args) {
        Ok(_) => Ok(sim.status("T-2")),
        Err(e) => Err(e.code),
    }
}

fn check(name: &str, tool: ToolName, args: Value, b: SessionBinding, expected: [Outcome; 10]) {
    for (f, want) in ALL.into_iter().zip(expected) {
        let mut sim = build(f);
        assert_eq!(sim.status("T-2"), fixture_status(f), "fixture {f:?}");
        let got = apply(&mut sim, b.clone(), tool, args.clone());
        assert_eq!(got, want, "{name} on {f:?}");
    }
}

#[test]
fn set_instruction() {
    let e = Err(InvalidState);
    check(
        "set_instruction",
        ToolName::SetInstruction,
        json!({"task_id": "T-2", "instruction": "i"}),
        orch(),
        // pending, awaiting, running, reported, checkpoint, h-agent, h-yhtye, merging, done, cancelled
        [Ok(Pending), Ok(Running), e, e, e, e, e, e, e, e],
    );
}

#[test]
fn modify_steps() {
    let e = Err(InvalidState);
    check(
        "modify_steps",
        ToolName::ModifySteps,
        json!({"task_id": "T-2", "steps": [{"kind": "implement"}]}),
        orch(),
        [
            Ok(Pending),
            Ok(AwaitingInstruction),
            Ok(Running),
            Ok(Running),
            Ok(Checkpoint),
            Ok(Handling),
            Ok(Handling),
            e,
            e,
            e,
        ],
    );
}

#[test]
fn resolve_checkpoint() {
    let e = Err(InvalidState);
    check(
        "resolve_checkpoint",
        ToolName::ResolveCheckpoint,
        json!({"task_id": "T-2", "decision": "continue"}),
        orch(),
        [e, e, e, e, Ok(Running), e, e, e, e, e],
    );
}

#[test]
fn cancel_task() {
    let e = Err(InvalidState);
    let c = Ok(Cancelled);
    check(
        "cancel_task",
        ToolName::CancelTask,
        json!({"task_id": "T-2", "reason": "r"}),
        orch(),
        [c, c, c, c, c, c, c, c, e, e],
    );
}

#[test]
fn report_step_done() {
    let e = Err(InvalidState);
    check(
        "report_step_done",
        ToolName::ReportStepDone,
        json!({"result": "r"}),
        sub("T-2", Role::Implementer, 0),
        [e, e, Ok(Running), e, e, e, e, e, e, e],
    );
}

#[test]
fn help() {
    let e = Err(InvalidState);
    check(
        "help",
        ToolName::Help,
        json!({"kind": "question", "message": "m"}),
        sub("T-2", Role::Implementer, 0),
        [e, e, Ok(Handling), e, e, e, e, e, e, e],
    );
}

#[test]
fn answer_help_resume_and_cancel() {
    // Only tasks with an open help accept answers; the rest have no H-1 for T-2.
    for f in [Fixture::HandlingAgent, Fixture::HandlingYhtye] {
        let help_id = |sim: &Sim| sim.state.open_help("T-2").expect("help").id.clone();
        let mut sim = build(f);
        let id = help_id(&sim);
        let got = apply(
            &mut sim,
            orch(),
            ToolName::AnswerHelp,
            json!({"help_id": id, "action": "resume", "reply": "go"}),
        );
        assert_eq!(got, Ok(Running), "resume on {f:?}");
        let mut sim = build(f);
        let id = help_id(&sim);
        let got = apply(
            &mut sim,
            orch(),
            ToolName::AnswerHelp,
            json!({"help_id": id, "action": "cancel_task"}),
        );
        assert_eq!(got, Ok(Cancelled), "cancel on {f:?}");
    }
}

#[test]
fn merge_result_after_cancel_is_ignored() {
    let mut sim = build(Fixture::Merging);
    sim.orch_ok(
        ToolName::CancelTask,
        json!({"task_id": "T-2", "reason": "r"}),
    );
    let before = sim.state.clone();
    sim.run(DomainCommand::GitDone {
        op: GitOp::FinishTask {
            group: "G-1".into(),
            task: "T-2".into(),
            kind: TaskKind::Code,
            message: "m".into(),
        },
        result: GitResult::Merged {
            detail: "late".into(),
        },
    })
    .expect("ok");
    assert_eq!(before, sim.state);
}
