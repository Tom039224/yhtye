//! Fake-agent tests: resume, startup failures, crashes and process cleanup.

mod common;

use std::time::Duration;

use common::{
    Session, assert_process_gone, fake_harness, next_event, start, start_err, wait_exited,
};
use serde_json::json;
use yhtye_core::acp::schema::{SessionId, StopReason};
use yhtye_core::acp::{
    AgentError, AgentEvent, AgentOutput, HarnessConfig, ModelSelect, SpawnOptions,
};

const GONE_TIMEOUT: Duration = Duration::from_secs(5);

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn resume(id: &str) -> SpawnOptions {
    SpawnOptions {
        resume: Some(SessionId::new(id)),
        ..SpawnOptions::default()
    }
}

#[tokio::test]
async fn resume_uses_session_load_and_replays_history() {
    let dir = tmp();
    let mut s = start(&fake_harness(json!({})), dir.path(), resume("prev-123")).await;
    let AgentEvent::Output(replayed @ AgentOutput::UserMessageChunk(_)) = s.next().await else {
        panic!("history replay must precede Ready")
    };
    assert_eq!(replayed.chunk_text(), Some("replayed history of prev-123"));
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("expected Ready")
    };
    assert!(info.resumed);
    assert_eq!(&*info.acp_session_id.0, "prev-123");
    assert_eq!(info.current_mode(), Some("bypassPermissions"));
    s.handle.shutdown().await;
}

#[tokio::test]
async fn resume_falls_back_to_new_session_without_load_capability() {
    let dir = tmp();
    let h = fake_harness(json!({"load_session": false}));
    let mut s = start(&h, dir.path(), resume("prev-123")).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("expected Ready")
    };
    assert!(!info.resumed);
    assert_ne!(&*info.acp_session_id.0, "prev-123");
    s.handle.shutdown().await;
}

#[tokio::test]
async fn shutdown_reaps_agent_and_its_children() {
    let dir = tmp();
    let h = fake_harness(json!({"turns": [{"actions": ["spawn_child", "wait_cancel"]}]}));
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let agent_pid = s.handle.pid().expect("agent pid");
    s.next().await;
    s.handle.prompt_text("spawn").expect("prompt");
    let text = s.until_message("child_pid:").await;
    let child_pid: u32 = text.trim_start_matches("child_pid:").parse().expect("pid");
    assert!(!common::process_gone(child_pid));

    // Shutdown during a turn: the turn is cancelled first, then the group is reaped.
    let Session {
        handle, mut events, ..
    } = s;
    handle.shutdown().await;
    let mut saw_cancelled = false;
    let mut last = None;
    while let Ok(ev) = events.try_recv() {
        saw_cancelled |= matches!(ev, AgentEvent::TurnEnded(Ok(StopReason::Cancelled)));
        if !matches!(ev, AgentEvent::Stderr(_)) {
            last = Some(ev);
        }
    }
    assert!(saw_cancelled, "running turn should end as cancelled");
    assert!(
        matches!(last, Some(AgentEvent::Exited { .. })),
        "Exited must be last: {last:?}"
    );
    assert_process_gone(agent_pid, GONE_TIMEOUT).await;
    assert_process_gone(child_pid, GONE_TIMEOUT).await;
}
#[tokio::test]
async fn dropping_the_handle_kills_the_process_group() {
    let dir = tmp();
    let h = fake_harness(json!({"turns": [{"actions": ["spawn_child", "wait_cancel"]}]}));
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let agent_pid = s.handle.pid().expect("agent pid");
    s.handle.prompt_text("spawn").expect("prompt");
    let text = s.until_message("child_pid:").await;
    let child_pid: u32 = text.trim_start_matches("child_pid:").parse().expect("pid");

    assert_eq!(
        common::group_members(agent_pid).len(),
        2,
        "agent + child in one group"
    );

    let Session {
        handle,
        mut events,
        timeout,
    } = s;
    drop(handle);
    wait_exited(&mut events, timeout).await;
    assert_process_gone(agent_pid, GONE_TIMEOUT).await;
    assert_process_gone(child_pid, GONE_TIMEOUT).await;
    common::assert_group_gone(agent_pid, GONE_TIMEOUT).await;
}

#[tokio::test]
async fn crash_mid_turn_ends_turn_with_error_and_reports_exit_code() {
    let dir = tmp();
    let h = fake_harness(json!({"turns": [{"actions": [{"message": "before"}, {"crash": 3}]}]}));
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;
    s.handle.prompt_text("crash").expect("prompt");
    let (_, stop) = s.turn().await;
    assert!(
        stop.is_err(),
        "turn must fail when the agent dies: {stop:?}"
    );
    assert_eq!(s.exited().await, (Some(3), None));
    assert_eq!(s.handle.prompt_text("again"), Err(AgentError::Closed));
    assert_eq!(
        s.handle.prompt_text("again"),
        Err(AgentError::Closed),
        "not Busy"
    );
}

#[tokio::test]
async fn missing_command_is_a_clear_spawn_error() {
    let dir = tmp();
    let h = HarnessConfig::plain("/nonexistent/yhtye-agent", vec![]);
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Spawn { command, message } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(command, "/nonexistent/yhtye-agent");
    assert!(message.contains("No such file"), "{message}");
}

#[tokio::test]
async fn agent_exiting_during_initialize_reports_step_code_and_stderr() {
    let dir = tmp();
    let h = fake_harness(json!({"exit_at": "initialize"}));
    let (err, mut events) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Startup { step, message } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "initialize");
    assert!(message.contains("exited with code 2"), "{message}");
    assert!(
        message.contains("exiting at initialize"),
        "stderr tail missing: {message}"
    );
    assert_eq!(
        wait_exited(&mut events, Duration::from_secs(5)).await.0,
        Some(2)
    );
}

#[tokio::test]
async fn failing_session_new_reports_the_agent_error() {
    let dir = tmp();
    let h = fake_harness(json!({"fail_at": "session/new"}));
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Startup { step, message } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/new");
    assert!(
        message.contains("scripted failure at session/new"),
        "{message}"
    );
    assert!(err.to_string().contains("session/new"));
}

#[tokio::test]
async fn hanging_startup_times_out_and_the_agent_is_killed() {
    let dir = tmp();
    let mut h = fake_harness(json!({"hang_at": "initialize"}));
    h.startup_timeout = Duration::from_millis(500);
    let (err, mut events) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    assert_eq!(
        err,
        AgentError::Timeout {
            step: "initialize".into(),
            millis: 500
        }
    );
    // The hung agent is terminated (by signal or after stdin EOF).
    let _ = wait_exited(&mut events, Duration::from_secs(10)).await;
}

#[tokio::test]
async fn unknown_mode_or_model_fails_startup() {
    let dir = tmp();
    let mut h = fake_harness(json!({}));
    h.mode_after_new = Some("yolo".into());
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    assert_eq!(
        err,
        AgentError::Unsupported {
            requested: "mode yolo".into(),
            available: "default, bypassPermissions".into()
        }
    );

    let h = fake_harness(json!({"models": ["default", "sonnet"]}));
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Startup { step, .. } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/set_config_option");
}

#[tokio::test]
async fn effort_is_set_after_the_model_and_verified() {
    let dir = tmp();
    let script = json!({
        "models": ["default", "haiku", "sonnet"],
        "efforts": {"sonnet": ["low", "high"]}
    });
    let effort = |value: &str| ModelSelect {
        config_id: "effort".into(),
        value: value.into(),
    };

    // The model comes first (the efforts belong to it), then the effort.
    let mut h = fake_harness(script.clone());
    h.model = Some(ModelSelect {
        config_id: "model".into(),
        value: "sonnet".into(),
    });
    h.effort = Some(effort("high"));
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("expected Ready")
    };
    assert_eq!(info.config_value("model"), Some("sonnet"));
    assert_eq!(info.config_value("effort"), Some("high"));
    s.handle.shutdown().await;

    // Without an effort the option stays at the agent's own default.
    h.effort = None;
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("expected Ready")
    };
    assert_eq!(info.config_value("effort"), Some("default"));
    s.handle.shutdown().await;

    // A value the model does not offer, and a model without the option,
    // fail the start instead of running with another effort.
    h.effort = Some(effort("max"));
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Startup { step, .. } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/set_config_option");
    let mut h = fake_harness(script);
    h.effort = Some(effort("high"));
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Startup { step, .. } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/set_config_option", "haiku has no effort");
}

#[tokio::test]
async fn events_after_startup_failure_end_with_exited() {
    let dir = tmp();
    let h = fake_harness(json!({"fail_at": "initialize"}));
    let (_, mut events) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let ev = next_event(&mut events, Duration::from_secs(5)).await;
    assert!(matches!(ev, AgentEvent::Exited { .. }), "{ev:?}");
}
