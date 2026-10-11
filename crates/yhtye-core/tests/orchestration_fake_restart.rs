//! Stopping the runtime mid-task and starting it again on the same database
//! (fake agents): in-flight tasks become `interrupted`, their sessions are
//! restored with `session/load` (or replaced by new ones), and the work completes.
//! The chats' orchestrators are not started with the project: they come back when
//! the chat is opened or has a notification to receive.

mod common;

use common::orch::ORCHESTRATOR_SESSION;

use std::path::Path;
use std::time::Duration;

use common::fake_harness;
use common::orch::{
    assert_gapless_from, assert_persisted, config, domain_events, is_message, is_task_status,
    prompts_to, shutdown_and_check, summary, until,
};
use serde_json::{Value, json};
use yhtye_core::acp::HarnessConfig;
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{DomainEvent, TaskStatus};
use yhtye_core::runtime::Orchestration;
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
    common::orch::send(&orch, "add a line").await;
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
    restart_with(dir, fake_harness(orch_script), fake_harness(impl_script)).await
}

async fn restart_with(
    dir: &Path,
    orchestrator: HarnessConfig,
    implementer: HarnessConfig,
) -> (Orchestration, Vec<ApiEvent>) {
    let cfg = config(dir, orchestrator, implementer, fake_harness(json!({})));
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

/// The ACP session `session`'s start gave up on, per start.
fn lost(events: &[ApiEvent], session: &str) -> Vec<Option<String>> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted {
                session: s,
                lost_acp_session_id,
                ..
            } if s == session => Some(lost_acp_session_id.clone()),
            _ => None,
        })
        .collect()
}

/// What the user was told went wrong with the start of `session`, with whether
/// another start followed (`retrying`).
fn failure_events(events: &[ApiEvent], session: &str) -> Vec<(String, bool)> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionFailed {
                session: s,
                error,
                retrying,
            } if s == session => Some((error.clone(), *retrying)),
            _ => None,
        })
        .collect()
}

/// What the user was told went wrong with the start of `session`.
fn failures(events: &[ApiEvent], session: &str) -> Vec<String> {
    failure_events(events, session)
        .into_iter()
        .map(|(error, _)| error)
        .collect()
}

/// Whether each failure of `session`'s start was followed by another start.
fn retrying(events: &[ApiEvent], session: &str) -> Vec<bool> {
    failure_events(events, session)
        .into_iter()
        .map(|(_, retrying)| retrying)
        .collect()
}

/// An implementer script whose agent fails (`fault`: `fail_at` / `hang_at`
/// at `session/set_mode`) in its first `times` processes, after restoring the
/// session fine, and works from then on.
fn flaky_set_mode(fault: &str, times: u32, attempts: &Path) -> Value {
    let script = json!({
        fault: "session/set_mode",
        "fault_attempts": times,
        "attempt_file": attempts.join("implementer").display().to_string(),
    });
    implementer_after_restart(script)
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

/// The restarted orchestrator was started only once its task had finished.
fn assert_orchestrator_started_after_the_task_settled(events: &[ApiEvent]) {
    let position = |f: &dyn Fn(&ApiEvent) -> bool| events.iter().position(f);
    let done = position(&|e| is_task_status(e, "T-1", TaskStatus::Done));
    let started = position(&|e| {
        matches!(&e.body, ApiEventBody::SessionStarted { session, .. }
            if session == ORCHESTRATOR_SESSION)
    });
    let (done, started) = (
        done.expect("the task is done"),
        started.expect("it started"),
    );
    assert!(
        done < started,
        "started at {started}, the task was done at {done}"
    );
}

#[tokio::test]
async fn restart_mid_step_restores_the_sessions_with_session_load() {
    let dir = tempfile::tempdir().expect("tempdir");
    let before = run_until_quit(dir.path()).await;
    let old_impl = started(&before, IMPLEMENTER);
    let old_orch = started(&before, ORCHESTRATOR_SESSION);
    assert_eq!(old_impl.len(), 1);
    assert_eq!(lost(&before, IMPLEMENTER), [None]);
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
    // Nothing was lost, nothing went wrong.
    assert_eq!(lost(&events, IMPLEMENTER), [None]);
    assert_eq!(failures(&events, IMPLEMENTER), Vec::<String>::new());
    let impl_prompts = prompts_to(&events, IMPLEMENTER);
    assert_eq!(impl_prompts.len(), 1, "{impl_prompts:?}");
    assert!(impl_prompts[0].starts_with("[yhtye:resume] task=T-1 step=1\n"));
    // The orchestrator was not started with the project: the notice that the
    // group settled started it, and its first prompt also says that its turn was
    // cut off by the quit.
    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert_eq!(orch_prompts.len(), 1, "{orch_prompts:?}");
    assert!(
        orch_prompts[0].starts_with("[yhtye:group_settled]"),
        "{orch_prompts:?}"
    );
    assert!(orch_prompts[0].contains("[yhtye:restarted]\n"));
    assert!(orch_prompts[0].contains("cut off"));
    assert_orchestrator_started_after_the_task_settled(&events);
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
    // The implementer is stopped in the background once its task is done, which
    // may be after the orchestrator's `finish_group`: wait for it to be recorded.
    let stopped = async {
        loop {
            let sessions = orch.store().sessions("P-1").await.expect("sessions");
            if sessions
                .iter()
                .any(|s| s.session_key == IMPLEMENTER && s.status == SessionStatus::Stopped)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(TIMEOUT, stopped)
        .await
        .expect("the implementer session is recorded as stopped");
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn restart_starts_new_sessions_when_session_load_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let before = run_until_quit(dir.path()).await;
    let old = [
        (ORCHESTRATOR_SESSION, started(&before, ORCHESTRATOR_SESSION)),
        (IMPLEMENTER, started(&before, IMPLEMENTER)),
    ];
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
            ApiEventBody::SessionFailed { session: s, error, .. }
                if s == session && error.contains("could not restore"))));
    }
    // The sessions that could not be restored are named, not forgotten.
    for (session, old) in &old {
        let old_id = old[0].0.clone();
        assert_eq!(lost(&events, session), [Some(old_id.clone())], "{session}");
        let told = failures(&events, session);
        assert!(told.iter().all(|t| t.contains(&old_id)), "{told:?}");
        // The new session started, so no failure was the last word.
        assert!(retrying(&events, session).iter().all(|r| *r), "{session}");
        let sessions = orch.store().sessions("P-1").await.expect("sessions");
        let record = sessions.iter().find(|r| r.session_key == *session);
        let record = record.expect("the session is recorded");
        assert_eq!(record.lost_acp_session_id.as_deref(), Some(old_id.as_str()));
        assert_ne!(
            record.acp_session_id, old_id,
            "{session} runs a new session"
        );
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
    assert!(
        orch_prompts[0].contains("task T-1 [done]"),
        "{orch_prompts:?}"
    );
    assert_orchestrator_started_after_the_task_settled(&events);
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn restart_with_an_agent_that_cannot_load_uses_the_full_step_prompt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let before = run_until_quit(dir.path()).await;
    let old_impl = started(&before, IMPLEMENTER);
    let (orch, events) = restart(
        dir.path(),
        orchestrator_after_restart(json!({})),
        implementer_after_restart(json!({"load_session": false})),
    )
    .await;
    assert_eq!(started(&events, IMPLEMENTER).len(), 1);
    assert!(!started(&events, IMPLEMENTER)[0].1);
    // The agent never tried to load it, but the old session id is kept anyway.
    assert_eq!(lost(&events, IMPLEMENTER), [Some(old_impl[0].0.clone())]);
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

#[tokio::test]
async fn a_restore_that_times_out_is_tried_again_with_a_new_agent_process() {
    let dir = tempfile::tempdir().expect("tempdir");
    let attempts = tempfile::tempdir().expect("tempdir");
    let before = run_until_quit(dir.path()).await;
    let old_impl = started(&before, IMPLEMENTER);

    // The first process loads the session, then never answers `set_mode`
    // (what stranded a real session): its start times out. The second works.
    let mut implementer = fake_harness(flaky_set_mode("hang_at", 1, attempts.path()));
    implementer.startup_timeout = Duration::from_secs(2);
    let orchestrator = fake_harness(orchestrator_after_restart(json!({})));
    let (orch, events) = restart_with(dir.path(), orchestrator, implementer).await;

    assert_task_done_after_interruption(&events);
    // The session came back with its own ACP session id: the conversation lives on.
    assert_eq!(
        started(&events, IMPLEMENTER),
        [(old_impl[0].0.clone(), true)]
    );
    assert_eq!(lost(&events, IMPLEMENTER), [None]);
    let told = failures(&events, IMPLEMENTER);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains("timed out at `session/set_mode`"),
        "{told:?}"
    );
    assert!(told[0].contains(&old_impl[0].0), "{told:?}");
    assert!(told[0].contains("trying again (1/2)"), "{told:?}");
    assert_eq!(
        retrying(&events, IMPLEMENTER),
        [true],
        "another start followed"
    );
    // It continued the step instead of redoing it.
    let impl_prompts = prompts_to(&events, IMPLEMENTER);
    assert_eq!(impl_prompts.len(), 1, "{impl_prompts:?}");
    assert!(impl_prompts[0].starts_with("[yhtye:resume] task=T-1 step=1\n"));
    let sessions = orch.store().sessions("P-1").await.expect("sessions");
    let record = sessions.iter().find(|r| r.session_key == IMPLEMENTER);
    assert_eq!(record.map(|r| r.lost_acp_session_id.clone()), Some(None));
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn when_every_try_fails_the_old_acp_session_id_is_kept() {
    let dir = tempfile::tempdir().expect("tempdir");
    let attempts = tempfile::tempdir().expect("tempdir");
    let before = run_until_quit(dir.path()).await;
    let old_id = started(&before, IMPLEMENTER)[0].0.clone();

    // The first try and both retries load the session and then fail at
    // `set_mode`; the fourth process (a new session) works.
    let implementer = fake_harness(flaky_set_mode("fail_at", 3, attempts.path()));
    let orchestrator = fake_harness(orchestrator_after_restart(json!({})));
    let (orch, events) = restart_with(dir.path(), orchestrator, implementer).await;

    assert_task_done_after_interruption(&events);
    let starts = started(&events, IMPLEMENTER);
    assert_eq!(starts.len(), 1, "{starts:?}");
    assert!(!starts[0].1, "a new session");
    assert_ne!(starts[0].0, old_id);
    // The new session knows which one it replaced...
    assert_eq!(lost(&events, IMPLEMENTER), [Some(old_id.clone())]);
    // ...and the user is told, with the id: each retry, then the fallback.
    let told = failures(&events, IMPLEMENTER);
    assert_eq!(told.len(), 3, "{told:?}");
    assert!(told[0].contains("trying again (1/2)"), "{told:?}");
    assert!(told[1].contains("trying again (2/2)"), "{told:?}");
    assert!(
        told[2].contains(&format!("could not restore the session {old_id}")),
        "{told:?}"
    );
    assert!(
        told.iter().all(|t| t.contains("session/set_mode")),
        "{told:?}"
    );
    // Every one of them was followed by another start (the last by the new session).
    assert_eq!(retrying(&events, IMPLEMENTER), [true, true, true]);
    // The step starts over in the new session.
    let impl_prompts = prompts_to(&events, IMPLEMENTER);
    assert!(impl_prompts[0].starts_with("[yhtye:step] task=T-1"));
    // The old id is in the database, next to the new one, after the task is done.
    let sessions = orch.store().sessions("P-1").await.expect("sessions");
    let record = sessions
        .iter()
        .find(|r| r.session_key == IMPLEMENTER)
        .expect("recorded");
    assert_eq!(record.acp_session_id, starts[0].0);
    assert_eq!(record.lost_acp_session_id.as_deref(), Some(old_id.as_str()));
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn only_the_failure_that_gives_the_start_up_is_final() {
    let dir = tempfile::tempdir().expect("tempdir");
    run_until_quit(dir.path()).await;

    // The agent cannot even be started: both retries of the restore fail, so
    // does the new session that replaces it.
    let cfg = config(
        dir.path(),
        fake_harness(orchestrator_after_restart(json!({}))),
        fake_harness(implementer_after_restart(json!({"fail_at": "initialize"}))),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("restarts");
    let mut events = Vec::new();
    until(&mut rx, &mut events, TIMEOUT, |e| {
        matches!(&e.body, ApiEventBody::SessionFailed { session, retrying: false, .. }
            if session == IMPLEMENTER)
    })
    .await;

    let told = failure_events(&events, IMPLEMENTER);
    assert_eq!(
        told.iter()
            .map(|(_, retrying)| *retrying)
            .collect::<Vec<_>>(),
        [true, true, true, false],
        "{told:?}"
    );
    assert!(told[3].0.contains("initialize"), "{told:?}");
    orch.shutdown().await;
}

/// The second run's implementer holds its resumed turn, so the task stays open
/// and nothing reaches the chat's orchestrator by itself.
fn held_implementer() -> Value {
    json!({"turns": [{"match": "[yhtye:resume]", "actions": ["wait_cancel"]}]})
}

/// Starts again on `dir` and waits until the interrupted task's implementer is
/// back, then lets a moment pass to catch a wrongly started orchestrator.
async fn restart_holding_the_task(
    dir: &Path,
) -> (
    Orchestration,
    tokio::sync::mpsc::UnboundedReceiver<ApiEvent>,
    Vec<ApiEvent>,
) {
    let cfg = config(
        dir,
        fake_harness(orchestrator_after_restart(json!({}))),
        fake_harness(held_implementer()),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("restarts");
    let mut events = Vec::new();
    until(&mut rx, &mut events, TIMEOUT, |e| {
        matches!(&e.body, ApiEventBody::SessionStarted { session, resumed: true, .. }
            if session == IMPLEMENTER)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    (orch, rx, events)
}

#[tokio::test]
async fn a_restart_spawns_no_orchestrator_until_the_chat_is_opened() {
    let dir = tempfile::tempdir().expect("tempdir");
    let before = run_until_quit(dir.path()).await;
    let old_orch = started(&before, ORCHESTRATOR_SESSION);
    let (orch, mut rx, mut events) = restart_holding_the_task(dir.path()).await;

    // The task's agent came back; the chat's orchestrator did not.
    assert!(
        started(&events, ORCHESTRATOR_SESSION).is_empty(),
        "{events:?}"
    );
    assert!(prompts_to(&events, ORCHESTRATOR_SESSION).is_empty());
    let sessions = orch.store().sessions("P-1").await.expect("sessions");
    let record = sessions
        .iter()
        .find(|s| s.session_key == ORCHESTRATOR_SESSION);
    assert_eq!(record.map(|r| r.status), Some(SessionStatus::Interrupted));

    // Opening the chat restores the orchestrator's own session, and it is told
    // that its turn was cut off.
    assert!(orch.open_chat("C-1").await.expect("open"), "it is starting");
    until(&mut rx, &mut events, TIMEOUT, |e| {
        is_message(e, ORCHESTRATOR_SESSION, "noted the restart")
    })
    .await;
    assert_eq!(
        started(&events, ORCHESTRATOR_SESSION),
        [(old_orch[0].0.clone(), true)]
    );
    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert_eq!(orch_prompts.len(), 1, "{orch_prompts:?}");
    assert!(orch_prompts[0].starts_with("[yhtye:restarted]\n"));
    assert!(orch_prompts[0].contains("cut off"));
    assert!(!orch.open_chat("C-1").await.expect("open again"), "it runs");
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_notification_for_a_chat_that_is_not_running_starts_it_and_reaches_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    run_until_quit(dir.path()).await;
    let (orch, mut rx, mut events) = restart_holding_the_task(dir.path()).await;
    assert!(started(&events, ORCHESTRATOR_SESSION).is_empty());

    // The user cancels the running task from the UI: the chat's orchestrator is
    // told, which starts it.
    orch.cancel_task("T-1", "not needed").await.expect("cancel");
    until(&mut rx, &mut events, TIMEOUT, |e| {
        matches!(&e.body, ApiEventBody::Prompted { session, .. } if session == ORCHESTRATOR_SESSION)
    })
    .await;
    assert_eq!(started(&events, ORCHESTRATOR_SESSION).len(), 1);
    let orch_prompts = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert_eq!(orch_prompts.len(), 1, "{orch_prompts:?}");
    assert!(
        orch_prompts[0].contains("the user cancelled task T-1"),
        "{orch_prompts:?}"
    );
    assert!(
        orch_prompts[0].contains("[yhtye:restarted]"),
        "{orch_prompts:?}"
    );
    shutdown_and_check(orch, &events).await;
}
