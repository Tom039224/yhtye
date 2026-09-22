//! Full orchestration loops with fake agents: the orchestrator calls tools over
//! MCP, the state machine starts sub-agent sessions, they report back, and the
//! orchestrator is woken through its inbox.

mod common;

use std::time::Duration;

use common::fake_harness;
use common::orch::{
    assert_gapless, config, domain_events, is_message, prompts_to, shutdown_and_check,
    started_sessions, summary, tool_calls, until,
};
use serde_json::{Value, json};
use yhtye_core::api::ApiEventBody;
use yhtye_core::domain::{DomainEvent, Role, TaskStatus};
use yhtye_core::runtime::{ORCHESTRATOR_SESSION, Orchestration};

const TIMEOUT: Duration = Duration::from_secs(20);

/// The orchestrator's planning turn: one group, one task with `steps`.
fn plan_turn(steps: Value) -> Value {
    json!({"match": "[yhtye:user_message]", "actions": [
        {"mcp_call": {"tool": "create_group", "args": {"title": "Add a line"}}},
        {"mcp_call": {"tool": "create_task", "args": {
            "group_id": "${group_id}", "title": "append", "kind": "code",
            "steps": steps, "instruction": "append hello to README.md"}}},
        {"message": "planned"}
    ]})
}

fn settled_turn() -> Value {
    json!({"match": "[yhtye:group_settled]", "actions": [
        {"message": "orchestrator-notified"},
        {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "done"}}}
    ]})
}

fn finished(e: &yhtye_core::api::ApiEvent) -> bool {
    is_message(e, ORCHESTRATOR_SESSION, "mcp:finish_group:")
}

#[tokio::test]
async fn create_task_spawns_sub_agent_whose_report_wakes_orchestrator() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut plan = plan_turn(json!([{"kind": "implement"}]));
    let extra = json!([
        "mcp_list",
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "not mine"}}}
    ]);
    if let (Some(actions), Some(extra)) = (plan["actions"].as_array_mut(), extra.as_array()) {
        actions.splice(0..0, extra.iter().cloned());
    }
    let orch_script = json!({"turns": [plan, settled_turn()]});
    let impl_script = json!({"turns": [{"match": "[yhtye:step]", "actions": [
        "mcp_list",
        {"mcp_call": {"tool": "create_group", "args": {"title": "not mine"}}},
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "added hello"}}}
    ]}]});
    let cfg = config(
        dir.path(),
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    orch.send_user_message("add a line to README")
        .expect("send");
    until(&mut rx, &mut events, TIMEOUT, finished).await;

    let msgs = summary(&events);
    let has = |needle: &str| msgs.iter().any(|m| m.contains(needle));
    for needle in [
        "orchestrator: mcp:tools:create_group,create_task,",
        "orchestrator: mcp:report_step_done:error:{\"error\":{\"code\":\"forbidden\"",
        "T-1/implementer: mcp:tools:report_step_done,help",
        "T-1/implementer: mcp:create_group:error:{\"error\":{\"code\":\"forbidden\"",
        "orchestrator: mcp:finish_group:ok:",
        "no git in this build (NoopGit)",
    ] {
        assert!(has(needle), "{needle} not in {msgs:#?}");
    }
    let calls = tool_calls(&events);
    for expected in [
        ("orchestrator", "create_group", true),
        ("orchestrator", "create_task", true),
        ("T-1/implementer", "report_step_done", true),
        ("orchestrator", "finish_group", true),
    ] {
        assert!(
            calls.contains(&(expected.0.into(), expected.1.into(), expected.2)),
            "{expected:?} not in {calls:?}"
        );
    }
    let step_prompts = prompts_to(&events, "T-1/implementer");
    assert_eq!(step_prompts.len(), 1);
    assert!(
        step_prompts[0].starts_with("[yhtye:step] task=T-1 kind=code step=1/2 step_kind=implement")
    );
    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert_eq!(
        orch_prompts[0],
        "[yhtye:user_message]\nadd a line to README"
    );
    assert_eq!(
        orch_prompts[1],
        "[yhtye:group_settled] group=G-1\nT-1 done: added hello"
    );
    assert!(events.iter().any(|e| matches!(&e.body,
        ApiEventBody::SessionStopped { session, .. } if session == "T-1/implementer")));
    let domain = domain_events(&events);
    assert!(domain.iter().any(|e| matches!(e,
        DomainEvent::TaskStatusChanged { task, status: TaskStatus::Done } if task == "T-1")));
    assert!(
        domain
            .iter()
            .any(|e| matches!(e, DomainEvent::GroupMergeFinished { ok: true, .. }))
    );
    assert_gapless(&events);
    let snapshot = orch.snapshot().await.expect("snapshot");
    assert!(snapshot.seq >= events.iter().filter(|e| !e.live).count() as u64);
    assert_eq!(snapshot.state.tasks[0].status, TaskStatus::Done);
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn help_is_answered_and_the_agent_resumes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let orch_script = json!({"turns": [
        plan_turn(json!([{"kind": "implement"}])),
        {"match": "[yhtye:help_raised]", "actions": [
            {"mcp_call": {"tool": "answer_help", "args": {
                "help_id": "${help_id}", "action": "resume", "reply": "use README.md"}}}
        ]},
        settled_turn()
    ]});
    let impl_script = json!({"turns": [
        {"match": "[yhtye:step]", "actions": [
            {"mcp_call": {"tool": "help", "args": {"kind": "question", "message": "which file?"}}}
        ]},
        {"match": "[yhtye:help_answer]", "actions": [
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "used README.md"}}}
        ]}
    ]});
    let cfg = config(
        dir.path(),
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    orch.send_user_message("add a line").expect("send");
    until(&mut rx, &mut events, TIMEOUT, finished).await;

    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert_eq!(
        orch_prompts[1],
        "[yhtye:help_raised] help_id=H-1 task=T-1 kind=question\nwhich file?"
    );
    assert_eq!(
        orch_prompts[2],
        "[yhtye:group_settled] group=G-1\nT-1 done: used README.md"
    );
    let impl_prompts = prompts_to(&events, "T-1/implementer");
    assert_eq!(
        impl_prompts[1],
        "[yhtye:help_answer] help_id=H-1\nuse README.md"
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn needs_changes_loops_through_fresh_reviewer_sessions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let orch_script = json!({"turns": [
        plan_turn(json!([{"kind": "implement"}, {"kind": "review"}])),
        settled_turn()
    ]});
    let impl_script = json!({"turns": [
        {"match": "step=3/5", "actions": [
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "added the test"}}}]},
        {"match": "[yhtye:step]", "actions": [
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "implemented"}}}]}
    ]});
    let review_script = json!({"turns": [
        {"match": "step=2/3", "actions": [
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "missing verdict"}}},
            {"mcp_call": {"tool": "report_step_done", "args": {
                "result": "please add a test", "verdict": "needs_changes"}}}]},
        {"match": "step=4/5", "actions": [
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "lgtm", "verdict": "approve"}}}]}
    ]});
    let cfg = config(
        dir.path(),
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(review_script),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    orch.send_user_message("add a line").expect("send");
    until(&mut rx, &mut events, TIMEOUT, finished).await;

    let sessions = started_sessions(&events);
    let count = |s: &str| sessions.iter().filter(|x| **x == s).count();
    assert_eq!(
        count("T-1/implementer"),
        1,
        "one implementer session: {sessions:?}"
    );
    assert_eq!(count("T-1/review-1"), 1, "{sessions:?}");
    assert_eq!(count("T-1/review-3"), 1, "{sessions:?}");
    let calls = tool_calls(&events);
    assert!(calls.contains(&("T-1/review-1".into(), "report_step_done".into(), false)));
    let impl_prompts = prompts_to(&events, "T-1/implementer");
    assert_eq!(impl_prompts.len(), 2);
    assert!(impl_prompts[1].contains("Address these findings:\nplease add a test"));
    let review_prompts = prompts_to(&events, "T-1/review-3");
    assert!(review_prompts[0].contains("step 2 (implement): added the test"));
    assert!(events.iter().any(|e| matches!(&e.body,
        ApiEventBody::SessionStopped { session, .. } if session == "T-1/review-1")));
    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert_eq!(
        orch_prompts[1],
        "[yhtye:group_settled] group=G-1\nT-1 done: lgtm"
    );
    assert!(
        domain_events(&events)
            .iter()
            .any(|e| matches!(e, DomainEvent::ReviewStepsInserted { after: 1, .. }))
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn checkpoint_wakes_orchestrator_and_note_reaches_the_next_step() {
    let dir = tempfile::tempdir().expect("tempdir");
    let orch_script = json!({"turns": [
        plan_turn(json!([{"kind": "implement"}, {"kind": "checkpoint"}, {"kind": "implement"}])),
        {"match": "[yhtye:checkpoint_reached]", "actions": [
            {"mcp_call": {"tool": "resolve_checkpoint", "args": {
                "task_id": "${task}", "decision": "continue", "note": "also add a newline"}}}]},
        settled_turn()
    ]});
    let impl_script = json!({"turns": [
        {"match": "also add a newline", "actions": [
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "second"}}}]},
        {"match": "[yhtye:step]", "actions": [
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "first"}}}]}
    ]});
    let cfg = config(
        dir.path(),
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    orch.send_user_message("add a line").expect("send");
    until(&mut rx, &mut events, TIMEOUT, finished).await;

    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert_eq!(
        orch_prompts[1],
        "[yhtye:checkpoint_reached] task=T-1 step=1\nstep 0 (implement): first"
    );
    assert_eq!(
        orch_prompts[2],
        "[yhtye:group_settled] group=G-1\nT-1 done: second"
    );
    let impl_prompts = prompts_to(&events, "T-1/implementer");
    assert!(impl_prompts[1].contains("Note from the orchestrator:\nalso add a newline"));
    assert_eq!(
        started_sessions(&events)
            .iter()
            .filter(|s| s.ends_with("implementer"))
            .count(),
        1
    );
    let roles: Vec<Role> = events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted { role, .. } => Some(*role),
            _ => None,
        })
        .collect();
    assert_eq!(roles, vec![Role::Orchestrator, Role::Implementer]);
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn failing_orchestrator_start_is_reported_and_leaves_nothing_behind() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(
        dir.path(),
        fake_harness(json!({"fail_at": "session/new"})),
        fake_harness(json!({})),
        fake_harness(json!({})),
    );
    let err = Orchestration::start(cfg).await.err().expect("start fails");
    assert!(err.to_string().contains("session/new"), "{err}");
}
