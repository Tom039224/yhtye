//! Fake-agent scenarios where something goes wrong or waits: reminders and
//! `protocol_violation`, crashes, start failures, dependencies with a late
//! instruction, and cancelling a task mid-turn.

mod common;

use std::time::Duration;

use common::fake_harness;
use common::orch::{
    config, is_message, is_task_status, prompts_to, shutdown_and_check, started_sessions, until,
};
use serde_json::{Value, json};
use yhtye_core::acp::AgentEvent;
use yhtye_core::acp::schema::StopReason;
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::TaskStatus;
use yhtye_core::runtime::{ORCHESTRATOR_SESSION, Orchestration};

const TIMEOUT: Duration = Duration::from_secs(20);

fn plan(tasks: &[Value]) -> Value {
    let mut actions = vec![json!({"mcp_call": {"tool": "create_group", "args": {"title": "g"}}})];
    for t in tasks {
        let mut args = json!({"group_id": "${group_id}", "title": "t", "kind": "code",
                              "steps": [{"kind": "implement"}], "instruction": "do it"});
        if let (Some(a), Some(extra)) = (args.as_object_mut(), t.as_object()) {
            a.extend(extra.clone());
        }
        actions.push(json!({"mcp_call": {"tool": "create_task", "args": args}}));
    }
    json!({"match": "[yhtye:user_message]\nplease", "actions": actions})
}

fn answer_help(reply: &str) -> Value {
    json!({"match": "[yhtye:help_raised]", "actions": [
        {"mcp_call": {"tool": "answer_help", "args": {
            "help_id": "${help_id}", "action": "resume", "reply": reply}}}]})
}

fn settled_turn() -> Value {
    json!({"match": "[yhtye:group_settled]", "actions": [
        {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "done"}}}]})
}

fn report(result: &str) -> Value {
    json!({"mcp_call": {"tool": "report_step_done", "args": {"result": result}}})
}

fn finished(e: &ApiEvent) -> bool {
    is_message(e, ORCHESTRATOR_SESSION, "mcp:finish_group:ok")
}

async fn run_until_finished(
    orch_script: Value,
    impl_script: Value,
) -> (Orchestration, Vec<ApiEvent>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(
        dir.path(),
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    orch.send_user_message("please").expect("send");
    until(&mut rx, &mut events, TIMEOUT, finished).await;
    (orch, events)
}

#[tokio::test]
async fn silent_turns_get_a_reminder_then_protocol_violation_help() {
    let orch_script =
        json!({"turns": [plan(&[json!({})]), answer_help("just report it"), settled_turn()]});
    let impl_script = json!({"turns": [
        {"match": "just report it", "actions": [report("reported after restart")]},
        {"match": "[yhtye:reminder]", "actions": [{"message": "still thinking"}]},
        {"match": "[yhtye:step]", "actions": [{"message": "thinking"}]}
    ]});
    let (orch, events) = run_until_finished(orch_script, impl_script).await;

    let impl_prompts = prompts_to(&events, "T-1/implementer");
    assert_eq!(impl_prompts.len(), 3, "{impl_prompts:#?}");
    assert!(impl_prompts[0].starts_with("[yhtye:step]"));
    assert!(impl_prompts[1].starts_with("[yhtye:reminder] task=T-1 step=1"));
    assert!(impl_prompts[2].contains("Note from the orchestrator:\njust report it"));
    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert!(
        orch_prompts[1]
            .starts_with("[yhtye:help_raised] help_id=H-1 task=T-1 kind=protocol_violation\n")
    );
    assert_eq!(
        orch_prompts[2],
        "[yhtye:group_settled] group=G-1\nT-1 done: reported after restart"
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn crashed_agent_raises_help_and_resume_starts_a_new_session() {
    let orch_script =
        json!({"turns": [plan(&[json!({})]), answer_help("retry please"), settled_turn()]});
    let impl_script = json!({"turns": [
        {"match": "retry please", "actions": [report("ok after crash")]},
        {"match": "[yhtye:step]", "actions": [{"crash": 3}]}
    ]});
    let (orch, events) = run_until_finished(orch_script, impl_script).await;

    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert!(
        orch_prompts[1]
            .starts_with("[yhtye:help_raised] help_id=H-1 task=T-1 kind=agent_crashed\n"),
        "{orch_prompts:#?}"
    );
    assert!(
        orch_prompts[1].contains("exit code Some(3)"),
        "{}",
        orch_prompts[1]
    );
    let starts = started_sessions(&events);
    assert_eq!(
        starts.iter().filter(|s| **s == "T-1/implementer").count(),
        2,
        "{starts:?}"
    );
    assert_eq!(
        orch_prompts[2],
        "[yhtye:group_settled] group=G-1\nT-1 done: ok after crash"
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn failing_sub_agent_start_raises_agent_crashed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let orch_script = json!({"turns": [
        plan(&[json!({})]),
        {"match": "[yhtye:help_raised]", "actions": [
            {"mcp_call": {"tool": "answer_help", "args": {"help_id": "${help_id}", "action": "cancel_task"}}}]},
        {"match": "[yhtye:group_settled]", "actions": [
            {"mcp_call": {"tool": "cancel_group", "args": {"group_id": "${group}", "reason": "failed"}}}]}
    ]});
    let cfg = config(
        dir.path(),
        fake_harness(orch_script),
        fake_harness(json!({"fail_at": "session/new"})),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    orch.send_user_message("please").expect("send");
    until(&mut rx, &mut events, TIMEOUT, |e| {
        is_message(e, ORCHESTRATOR_SESSION, "mcp:cancel_group:ok")
    })
    .await;
    assert!(events.iter().any(|e| matches!(&e.body,
        ApiEventBody::SessionFailed { session, error }
            if session == "T-1/implementer" && error.contains("session/new"))));
    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert!(
        orch_prompts[1].contains("kind=agent_crashed\nThe implementer agent could not be started")
    );
    assert!(orch_prompts[2].starts_with("[yhtye:group_settled] group=G-1\nT-1 cancelled"));
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn dependency_with_empty_instruction_asks_orchestrator_when_resolved() {
    let orch_script = json!({"turns": [
        plan(&[json!({"instruction": "first"}), json!({"depends_on": ["T-1"], "instruction": null})]),
        {"match": "[yhtye:instruction_needed]", "actions": [
            {"mcp_call": {"tool": "set_instruction", "args": {
                "task_id": "${task}", "instruction": "second, using T-1's result"}}}]},
        settled_turn()
    ]});
    let impl_script = json!({"turns": [
        {"match": "task=T-2", "actions": [report("second done")]},
        {"match": "task=T-1", "actions": [report("first done")]}
    ]});
    let (orch, events) = run_until_finished(orch_script, impl_script).await;

    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert_eq!(
        orch_prompts[1],
        "[yhtye:instruction_needed] task=T-2\nT-1: first done"
    );
    assert_eq!(
        orch_prompts[2],
        "[yhtye:group_settled] group=G-1\nT-1 done: first done\nT-2 done: second done"
    );
    let t2 = prompts_to(&events, "T-2/implementer");
    assert!(t2[0].contains("second, using T-1's result"));
    let order: Vec<usize> = ["T-1", "T-2"]
        .iter()
        .map(|t| {
            events
                .iter()
                .position(|e| is_task_status(e, t, TaskStatus::Done))
                .expect("done")
        })
        .collect();
    assert!(order[0] < order[1]);
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn cancel_task_stops_the_agent_mid_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let orch_script = json!({"turns": [
        plan(&[json!({})]),
        {"match": "cancel it", "actions": [
            {"mcp_call": {"tool": "cancel_task", "args": {"task_id": "T-1", "reason": "user asked"}}}]},
        {"match": "[yhtye:group_settled]", "actions": [
            {"mcp_call": {"tool": "cancel_group", "args": {"group_id": "${group}", "reason": "user asked"}}}]}
    ]});
    let impl_script = json!({"turns": [{"match": "[yhtye:step]", "actions": ["wait_cancel"]}]});
    let cfg = config(
        dir.path(),
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    orch.send_user_message("please").expect("send");
    until(&mut rx, &mut events, TIMEOUT, |e| {
        matches!(&e.body, ApiEventBody::Prompted { session, .. } if session == "T-1/implementer")
    })
    .await;
    orch.send_user_message("cancel it").expect("send");
    until(&mut rx, &mut events, TIMEOUT, |e| {
        matches!(&e.body, ApiEventBody::SessionStopped { session, .. } if session == "T-1/implementer")
    })
    .await;
    assert!(
        events
            .iter()
            .any(|e| is_task_status(e, "T-1", TaskStatus::Cancelled))
    );
    assert!(
        events.iter().any(|e| matches!(&e.body,
            ApiEventBody::Agent { session, event: AgentEvent::TurnEnded(Ok(StopReason::Cancelled)) }
                if session == "T-1/implementer")),
        "the running turn is cancelled before the session stops"
    );
    until(&mut rx, &mut events, TIMEOUT, |e| {
        is_message(e, ORCHESTRATOR_SESSION, "mcp:cancel_group:ok")
    })
    .await;
    let snapshot = orch.snapshot().await.expect("snapshot");
    assert_eq!(snapshot.state.tasks[0].status, TaskStatus::Cancelled);
    assert_eq!(
        snapshot.state.tasks[0].cancel_reason.as_deref(),
        Some("user asked")
    );
    shutdown_and_check(orch, &events).await;
}
