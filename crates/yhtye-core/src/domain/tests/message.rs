//! `message_task`: what the state machine accepts, refuses and logs
//! (`mcp-tools.md` §3).

use serde_json::{Value, json};

use super::*;

fn running() -> Sim {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    sim.deliver_inbox();
    sim
}

fn message(task: &str, text: &str) -> Value {
    json!({"task_id": task, "text": text})
}

fn deliveries(chain: &Chain) -> Vec<(AgentRef, String)> {
    chain
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::DeliverMessage { agent, text } => Some((agent.clone(), text.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_running_step_gets_the_message_through_the_runtime() {
    let mut sim = running();
    let before = sim.state.clone();
    let chain = sim
        .tool(orch(), ToolName::MessageTask, message("t1", "use tabs"))
        .expect("accepted");
    assert!(
        chain.reply.is_none(),
        "answered when the runtime settles it"
    );
    assert_eq!(
        deliveries(&chain),
        vec![(agent("t1", Role::Implementer, 0), "use tabs".into())]
    );
    assert_eq!(
        chain.events,
        vec![DomainEvent::TaskMessageSent {
            task: "t1".into(),
            step: 0,
            text: "use tabs".into(),
        }]
    );
    assert_eq!(sim.state, before, "the log entry changes no state");
}

#[test]
fn the_runtime_settling_a_message_answers_the_call() {
    let mut sim = running();
    for (outcome, via, detail) in [
        (
            MessageOutcome::Delivered,
            Some(MessageVia::Steering),
            "injected",
        ),
        (MessageOutcome::Returned, None, "agent not running"),
    ] {
        let chain = sim
            .run(DomainCommand::MessageResolved {
                task: "t1".into(),
                step: 0,
                outcome,
                via,
                detail: detail.into(),
            })
            .expect("ok");
        let reply = chain.reply.expect("reply").expect("ok reply");
        assert_eq!(
            reply,
            json!({"task_id": "t1", "step": 0, "outcome": outcome, "via": via, "detail": detail})
        );
        assert_eq!(
            chain.events,
            vec![DomainEvent::TaskMessageResolved {
                task: "t1".into(),
                step: 0,
                outcome,
                via,
                detail: detail.into(),
            }]
        );
    }
}

#[test]
fn a_review_step_can_be_messaged_too() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({"steps": [{"kind": "implement"}, {"kind": "review"}]}));
    sim.deliver_inbox();
    sim.report("t1", Role::Implementer, None);
    assert_eq!(sim.t("t1").current, 1);
    let chain = sim
        .tool(
            orch(),
            ToolName::MessageTask,
            message("t1", "also check the docs"),
        )
        .expect("accepted");
    assert_eq!(
        deliveries(&chain),
        vec![(agent("t1", Role::Reviewer, 1), "also check the docs".into())]
    );
}

#[test]
fn a_step_that_already_reported_gets_the_message_back_at_once() {
    let mut sim = running();
    sim.sub_ok(
        sub("t1", Role::Implementer, 0),
        ToolName::ReportStepDone,
        json!({"result": "done"}),
    );
    let chain = sim
        .tool(
            orch(),
            ToolName::MessageTask,
            message("t1", "one more thing"),
        )
        .expect("answered");
    assert!(deliveries(&chain).is_empty());
    let reply = chain.reply.expect("reply").expect("ok reply");
    assert_eq!(reply["outcome"], "returned");
    assert_eq!(reply["via"], Value::Null);
    assert_eq!(reply["step"], 0);
    let detail = reply["detail"].as_str().expect("detail");
    assert!(detail.contains("already reported step 1 done"), "{detail}");
    assert!(detail.contains("modify_steps"), "{detail}");
    assert!(matches!(
        chain.events.as_slice(),
        [
            DomainEvent::TaskMessageSent { .. },
            DomainEvent::TaskMessageResolved {
                outcome: MessageOutcome::Returned,
                via: None,
                ..
            }
        ]
    ));
}

#[test]
fn a_step_that_has_not_started_gets_the_message_back_at_once() {
    let mut sim = Sim::new();
    sim.group();
    // The workspace is never answered: the task runs, its first step has not started.
    let create = sim.tool_cmd(
        orch(),
        ToolName::CreateTask,
        json!({"group_id": "g1", "title": "t", "kind": "code",
               "steps": [{"kind": "implement"}], "instruction": "do it"}),
    );
    sim.step(create).expect("created");
    assert_eq!(sim.status("t1"), TaskStatus::Running);
    let chain = sim
        .tool(orch(), ToolName::MessageTask, message("t1", "hi"))
        .expect("answered");
    let reply = chain.reply.expect("reply").expect("ok reply");
    assert_eq!(reply["outcome"], "returned");
    assert!(
        reply["detail"]
            .as_str()
            .expect("detail")
            .contains("not started")
    );
}

#[test]
fn the_text_must_not_be_empty() {
    let mut sim = running();
    for text in ["", "  \n"] {
        assert_eq!(
            sim.orch_err(ToolName::MessageTask, message("t1", text)),
            ErrorCode::InvalidArgument
        );
    }
}

#[test]
fn unknown_tasks_and_other_chats_are_refused() {
    let mut sim = running();
    assert_eq!(
        sim.orch_err(ToolName::MessageTask, message("t9", "hi")),
        ErrorCode::NotFound
    );
    let other = sim.chat("feature");
    let err = sim
        .tool(orch_of(&other), ToolName::MessageTask, message("t1", "hi"))
        .expect_err("another chat's task");
    assert_eq!(err.code, ErrorCode::Forbidden);
}

#[test]
fn a_second_message_while_one_is_being_delivered_conflicts() {
    let mut sim = running();
    let call = parse_call(
        ToolName::MessageTask,
        message("t1", "again").as_object().cloned(),
    )
    .expect("args");
    let crate::mcp::ToolCall::MessageTask(args) = call else {
        panic!("wrong call")
    };
    let err = sim
        .run(DomainCommand::MessageTask {
            binding: orch(),
            args,
            pending: true,
        })
        .expect_err("busy");
    assert_eq!(err.code, ErrorCode::Conflict);
}

#[test]
fn other_states_say_what_to_do_instead() {
    let advice = [
        ("pending", "set_instruction"),
        ("checkpoint", "resolve_checkpoint"),
        ("handling", "answer_help"),
        ("done", "create_task"),
        ("cancelled", "create_task"),
    ];
    for (status, hint) in advice {
        let mut sim = Sim::new();
        sim.group();
        match status {
            "pending" => {
                sim.task(json!({"instruction": null}));
            }
            "checkpoint" => {
                sim.task(json!({"steps": [{"kind": "checkpoint"}]}));
            }
            _ => {
                sim.task(json!({}));
            }
        }
        match status {
            "handling" => {
                sim.sub_ok(
                    sub("t1", Role::Implementer, 0),
                    ToolName::Help,
                    json!({"kind": "question", "message": "?"}),
                );
            }
            "done" => {
                sim.report("t1", Role::Implementer, None);
            }
            "cancelled" => {
                sim.orch_ok(
                    ToolName::CancelTask,
                    json!({"task_id": "t1", "reason": "r"}),
                );
            }
            _ => {}
        }
        let err = sim
            .tool(orch(), ToolName::MessageTask, message("t1", "hi"))
            .expect_err("not running");
        assert_eq!(err.code, ErrorCode::InvalidState, "{status}");
        assert!(err.message.contains(hint), "{status}: {}", err.message);
        assert!(
            err.message
                .contains(&format!("is {}", sim.status("t1").as_str()))
        );
    }
}

#[test]
fn the_hints_cover_every_status() {
    use TaskStatus::*;
    for status in [
        Pending,
        AwaitingInstruction,
        Running,
        Checkpoint,
        Handling,
        Merging,
        Interrupted,
        Done,
        Cancelled,
    ] {
        assert!(!unavailable_hint(status).is_empty(), "{status:?}");
    }
}
