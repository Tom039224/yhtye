//! The whole Stage 2 loop with fake agents: the orchestrator calls tools over MCP,
//! Yhtye starts sub-agent sessions, sub-agents report back, the orchestrator is woken.

mod common;

use std::time::Duration;

use common::fake_harness;
use common::orch::{is_message, prompts_to, shutdown_and_check, summary, tool_calls, until};
use serde_json::{Value, json};
use yhtye_core::domain::Role;
use yhtye_core::runtime::{ORCHESTRATOR_SESSION, OrchEvent, Orchestration, OrchestrationConfig};

const TIMEOUT: Duration = Duration::from_secs(20);

fn config(
    dir: &std::path::Path,
    orch: Value,
    implementer: Value,
    reviewer: Value,
) -> OrchestrationConfig {
    OrchestrationConfig {
        project: "P-1".into(),
        project_dir: dir.to_path_buf(),
        base_branch: "main".into(),
        orchestrator: fake_harness(orch),
        implementer: fake_harness(implementer),
        reviewer: fake_harness(reviewer),
        mcp_bind: "127.0.0.1:0".parse().expect("addr"),
    }
}

fn plan_turn(steps: Value) -> Value {
    json!({"match": "[yhtye:user_message]", "actions": [
        "mcp_list",
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "not mine"}}},
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

#[tokio::test]
async fn create_task_spawns_sub_agent_whose_report_wakes_orchestrator() {
    let dir = tempfile::tempdir().expect("tempdir");
    let orch_script = json!({"turns": [plan_turn(json!([{"kind": "implement"}])), settled_turn()]});
    let impl_script = json!({"turns": [{"match": "[yhtye:step]", "actions": [
        "mcp_list",
        {"mcp_call": {"tool": "create_group", "args": {"title": "not mine"}}},
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "added hello"}}}
    ]}]});
    let cfg = config(dir.path(), orch_script, impl_script, json!({}));
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    orch.send_user_message("add a line to README")
        .expect("send");
    until(&mut rx, &mut events, TIMEOUT, |e| {
        is_message(e, ORCHESTRATOR_SESSION, "mcp:finish_group:")
    })
    .await;

    let msgs = summary(&events);
    let has = |needle: &str| msgs.iter().any(|m| m.contains(needle));
    assert!(
        has("orchestrator: mcp:tools:create_group,create_task,"),
        "{msgs:#?}"
    );
    assert!(
        has("orchestrator: mcp:report_step_done:error:{\"error\":{\"code\":\"forbidden\""),
        "{msgs:#?}"
    );
    assert!(
        has("T-1/implementer: mcp:tools:report_step_done,help"),
        "{msgs:#?}"
    );
    assert!(
        has("T-1/implementer: mcp:create_group:error:{\"error\":{\"code\":\"forbidden\""),
        "{msgs:#?}"
    );
    assert!(has("orchestrator: mcp:finish_group:ok:"), "{msgs:#?}");

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
    assert!(events.iter().any(|e| matches!(e,
        OrchEvent::SessionStarted { session, role: Role::Implementer, task: Some(t), .. }
            if session == "T-1/implementer" && t == "T-1")));
    let step_prompts = prompts_to(&events, "T-1/implementer");
    assert_eq!(step_prompts.len(), 1);
    assert!(
        step_prompts[0].starts_with("[yhtye:step] task=T-1 kind=code step=1/2 step_kind=implement")
    );
    assert!(step_prompts[0].contains("append hello to README.md"));
    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert_eq!(
        orch_prompts[0],
        "[yhtye:user_message]\nadd a line to README"
    );
    assert_eq!(
        orch_prompts[1],
        "[yhtye:group_settled] group=G-1\nT-1 done: added hello"
    );
    assert!(
        events.iter().any(
            |e| matches!(e, OrchEvent::SessionStopped { session } if session == "T-1/implementer")
        ),
        "the implementer session stops when its task is done"
    );
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
    let cfg = config(dir.path(), orch_script, impl_script, json!({}));
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    orch.send_user_message("add a line").expect("send");
    until(&mut rx, &mut events, TIMEOUT, |e| {
        is_message(e, ORCHESTRATOR_SESSION, "mcp:finish_group:ok")
    })
    .await;

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
async fn review_step_runs_in_a_fresh_reviewer_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let orch_script = json!({"turns": [
        plan_turn(json!([{"kind": "implement"}, {"kind": "review"}])),
        settled_turn()
    ]});
    let impl_script = json!({"turns": [{"match": "[yhtye:step]", "actions": [
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "implemented"}}}
    ]}]});
    let review_script = json!({"turns": [{"match": "step_kind=review", "actions": [
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "missing verdict"}}},
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "lgtm", "verdict": "approve"}}}
    ]}]});
    let cfg = config(dir.path(), orch_script, impl_script, review_script);
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    orch.send_user_message("add a line").expect("send");
    until(&mut rx, &mut events, TIMEOUT, |e| {
        is_message(e, ORCHESTRATOR_SESSION, "mcp:finish_group:ok")
    })
    .await;

    assert!(events.iter().any(|e| matches!(e,
        OrchEvent::SessionStarted { session, role: Role::Reviewer, .. } if session == "T-1/review-1")));
    let calls = tool_calls(&events);
    assert!(calls.contains(&("T-1/review-1".into(), "report_step_done".into(), false)));
    assert!(calls.contains(&("T-1/review-1".into(), "report_step_done".into(), true)));
    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert_eq!(
        orch_prompts[1],
        "[yhtye:group_settled] group=G-1\nT-1 done: lgtm"
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn failing_orchestrator_start_is_reported_and_leaves_nothing_behind() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(
        dir.path(),
        json!({"fail_at": "session/new"}),
        json!({}),
        json!({}),
    );
    let err = Orchestration::start(cfg).await.err().expect("start fails");
    assert!(err.to_string().contains("session/new"), "{err}");
}
