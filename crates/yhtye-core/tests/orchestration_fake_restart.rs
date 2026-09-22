//! Stopping the runtime mid-task and starting it again on the same database
//! (fake agents): in-flight tasks become `interrupted`, their sessions are
//! restored with `session/load` (or replaced by new ones), and the work completes.

mod common;

use std::path::Path;
use std::time::Duration;

use common::fake_harness;
use common::orch::{
    assert_gapless_from, assert_persisted, config, domain_events, is_message, is_task_status,
    prompts_to, shutdown_and_check, summary, until,
};
use serde_json::{Value, json};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{DomainEvent, TaskStatus};
use yhtye_core::runtime::{ORCHESTRATOR_SESSION, Orchestration};
use yhtye_core::store::SessionStatus;

const TIMEOUT: Duration = Duration::from_secs(20);
const IMPLEMENTER: &str = "T-1/implementer";

/// First run: the orchestrator plans a one-step task and keeps its turn open;
/// the implementer starts working and is still in its turn at the "quit".
async fn run_until_quit(dir: &Path) -> Vec<ApiEvent> {
    let orch_script = json!({"turns": [{"match": "[yhtye:user_message]", "actions": [
        {"mcp_call": {"tool": "create_group", "args": {"title": "Add a line"}}},
        {"mcp_call": {"tool": "create_task", "args": {
            "group_id": "${group_id}", "title": "append", "kind": "code",
            "steps": [{"kind": "implement"}], "instruction": "append hello to README.md"}}},
        {"message": "planned"},
        "wait_cancel"
    ]}]});
    let impl_script = json!({"turns": [{"match": "[yhtye:step]", "actions": [
        {"message": "working on it"}, "wait_cancel"
    ]}]});
    let cfg = config(
        dir,
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    orch.send_user_message("add a line").expect("send");
    until(&mut rx, &mut events, TIMEOUT, |e| {
        is_message(e, IMPLEMENTER, "working on it")
    })
    .await;
    assert_persisted(&orch).await;
    shutdown_and_check(orch, &events).await; // the simulated app quit
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    events
}

/// Second run's orchestrator: acknowledges the restart notice, then finishes.
fn orchestrator_after_restart(extra: Value) -> Value {
    let mut script = json!({"turns": [
        {"match": "[yhtye:group_settled]", "actions": [
            {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "done"}}}
        ]},
        {"match": "[yhtye:restarted]", "actions": [{"message": "noted the restart"}]}
    ]});
    if let (Some(s), Some(e)) = (script.as_object_mut(), extra.as_object()) {
        s.extend(e.clone());
    }
    script
}

fn implementer_after_restart(extra: Value) -> Value {
    let mut script = json!({"turns": [
        {"match": "[yhtye:resume]", "actions": [
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "resumed and done"}}}
        ]},
        {"match": "[yhtye:step]", "actions": [
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "redone in a new session"}}}
        ]}
    ]});
    if let (Some(s), Some(e)) = (script.as_object_mut(), extra.as_object()) {
        s.extend(e.clone());
    }
    script
}

async fn restart(
    dir: &Path,
    orch_script: Value,
    impl_script: Value,
) -> (Orchestration, Vec<ApiEvent>) {
    let cfg = config(
        dir,
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("restarts");
    let mut events = Vec::new();
    until(&mut rx, &mut events, TIMEOUT, |e| {
        is_message(e, ORCHESTRATOR_SESSION, "mcp:finish_group:ok")
    })
    .await;
    (orch, events)
}

fn started(events: &[ApiEvent], session: &str) -> Vec<(String, bool)> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted {
                session: s,
                acp_session_id,
                resumed,
                ..
            } if s == session => Some((acp_session_id.clone(), *resumed)),
            _ => None,
        })
        .collect()
}

fn interrupted(events: &[ApiEvent]) -> Vec<&str> {
    let mut sessions: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionInterrupted { session } => Some(session.as_str()),
            _ => None,
        })
        .collect();
    sessions.sort_unstable();
    sessions
}

fn last_seq(events: &[ApiEvent]) -> u64 {
    events.last().map_or(0, |e| e.seq)
}

fn assert_task_done_after_interruption(events: &[ApiEvent]) {
    let statuses: Vec<TaskStatus> = domain_events(events)
        .into_iter()
        .filter_map(|e| match e {
            DomainEvent::TaskStatusChanged { task, status } if task == "T-1" => Some(*status),
            _ => None,
        })
        .collect();
    assert_eq!(
        statuses,
        [
            TaskStatus::Interrupted,
            TaskStatus::Running,
            TaskStatus::Merging,
            TaskStatus::Done
        ]
    );
    assert!(
        events
            .iter()
            .any(|e| is_task_status(e, "T-1", TaskStatus::Done))
    );
}

#[tokio::test]
async fn restart_mid_step_restores_the_sessions_with_session_load() {
    let dir = tempfile::tempdir().expect("tempdir");
    let before = run_until_quit(dir.path()).await;
    let old_impl = started(&before, IMPLEMENTER);
    let old_orch = started(&before, ORCHESTRATOR_SESSION);
    assert_eq!(old_impl.len(), 1);
    assert!(before.iter().any(|e| matches!(&e.body,
        ApiEventBody::SessionStopped { session, suspended: true } if session == IMPLEMENTER)));

    let (orch, events) = restart(
        dir.path(),
        orchestrator_after_restart(json!({})),
        implementer_after_restart(json!({})),
    )
    .await;
    // Numbering continues where the first run stopped.
    assert_gapless_from(&events, last_seq(&before) + 1);
    assert_eq!(interrupted(&events), [IMPLEMENTER, ORCHESTRATOR_SESSION]);
    assert_task_done_after_interruption(&events);
    // Both sessions came back with their old ACP session ids via session/load.
    assert_eq!(
        started(&events, IMPLEMENTER),
        [(old_impl[0].0.clone(), true)]
    );
    assert_eq!(
        started(&events, ORCHESTRATOR_SESSION),
        [(old_orch[0].0.clone(), true)]
    );
    let impl_prompts = prompts_to(&events, IMPLEMENTER);
    assert_eq!(impl_prompts.len(), 1, "{impl_prompts:?}");
    assert!(impl_prompts[0].starts_with("[yhtye:resume] task=T-1 step=1\n"));
    // The orchestrator's turn was cut off by the quit: it is told so.
    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert!(
        orch_prompts[0].starts_with("[yhtye:restarted]\n"),
        "{orch_prompts:?}"
    );
    assert!(orch_prompts[0].contains("cut off"));
    // History replayed by session/load is not published again.
    let msgs = summary(&events);
    assert!(
        !msgs.iter().any(|m| m.contains("replayed history")),
        "{msgs:#?}"
    );
    assert!(msgs.iter().any(|m| m.contains("mcp:report_step_done:ok")));

    // The first run's output is in the log as one coalesced block.
    let history = orch
        .store()
        .session_events("P-1", IMPLEMENTER)
        .await
        .expect("history");
    let texts: Vec<(String, String)> = history
        .iter()
        .filter(|e| e.kind == "agent_text")
        .map(|e| (e.body["kind"].to_string(), e.body["text"].to_string()))
        .collect();
    assert_eq!(texts[0], ("\"message\"".into(), "\"working on it\"".into()));
    let sessions = orch.store().sessions("P-1").await.expect("sessions");
    assert!(
        sessions
            .iter()
            .any(|s| s.session_key == IMPLEMENTER && s.status == SessionStatus::Stopped)
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn restart_starts_new_sessions_when_session_load_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let before = run_until_quit(dir.path()).await;
    let fail_load = json!({"fail_at": "session/load"});
    let (orch, events) = restart(
        dir.path(),
        orchestrator_after_restart(fail_load.clone()),
        implementer_after_restart(fail_load),
    )
    .await;
    assert_gapless_from(&events, last_seq(&before) + 1);
    assert_task_done_after_interruption(&events);
    for session in [ORCHESTRATOR_SESSION, IMPLEMENTER] {
        let starts = started(&events, session);
        assert_eq!(starts.len(), 1, "{session}: {starts:?}");
        assert!(!starts[0].1, "{session} is a new session");
        assert!(events.iter().any(|e| matches!(&e.body,
            ApiEventBody::SessionFailed { session: s, error }
                if s == session && error.contains("could not restore"))));
    }
    // The new implementer session gets the whole step again, with a note.
    let impl_prompts = prompts_to(&events, IMPLEMENTER);
    assert_eq!(impl_prompts.len(), 1);
    assert!(impl_prompts[0].starts_with("[yhtye:step] task=T-1"));
    assert!(impl_prompts[0].contains("could not be restored"));
    // The new orchestrator session gets a summary of the state.
    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert!(
        orch_prompts[0].contains("could not be restored"),
        "{orch_prompts:?}"
    );
    assert!(orch_prompts[0].contains("task T-1 [interrupted] step 1/2: append"));
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn restart_with_an_agent_that_cannot_load_uses_the_full_step_prompt() {
    let dir = tempfile::tempdir().expect("tempdir");
    run_until_quit(dir.path()).await;
    let (orch, events) = restart(
        dir.path(),
        orchestrator_after_restart(json!({})),
        implementer_after_restart(json!({"load_session": false})),
    )
    .await;
    assert_eq!(started(&events, IMPLEMENTER).len(), 1);
    assert!(!started(&events, IMPLEMENTER)[0].1);
    let impl_prompts = prompts_to(&events, IMPLEMENTER);
    assert!(impl_prompts[0].starts_with("[yhtye:step] task=T-1"));
    assert!(
        summary(&events)
            .iter()
            .any(|m| m.contains("redone in a new session"))
    );
    assert_task_done_after_interruption(&events);
    shutdown_and_check(orch, &events).await;
}
