//! Fake-agent tests: resume, startup failures, crashes and process cleanup.

mod common;

use std::time::Duration;

use common::{
    Session, assert_process_gone, fake_harness, message_text, next_event, start, start_err,
    wait_exited,
};
use serde_json::json;
use yhtye_core::acp::schema::{SessionId, StopReason};
use yhtye_core::acp::{
    AgentError, AgentEvent, AgentOutput, HarnessConfig, ModelSelect, SpawnOptions, StartupStep,
};

const GONE_TIMEOUT: Duration = Duration::from_secs(5);

fn step_names(steps: &[StartupStep]) -> Vec<&str> {
    steps.iter().map(|s| s.step.as_str()).collect()
}

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
    let AgentError::Startup { step, message, .. } = &err else {
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
    let AgentError::Startup { step, message, .. } = &err else {
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
    let AgentError::Timeout {
        step,
        millis,
        stderr,
        steps,
    } = &err
    else {
        panic!("{err:?}")
    };
    assert_eq!((step.as_str(), *millis), ("initialize", 500));
    // The timed out step is the last one, with the time it waited.
    assert_eq!(
        step_names(steps.as_deref().unwrap_or_default()),
        ["initialize"]
    );
    assert!(steps.as_deref().is_some_and(|s| s[0].millis >= 500));
    // What the hung agent said on stderr is in the error.
    assert!(
        stderr
            .as_deref()
            .is_some_and(|t| t.contains("fake: hanging at initialize as scripted")),
        "{stderr:?}"
    );
    assert!(
        err.to_string().ends_with(
            "after 500 ms\n--- agent stderr (tail) ---\nfake: hanging at initialize as scripted"
        ),
        "{err}"
    );
    // The hung agent is terminated (by signal or after stdin EOF).
    let _ = wait_exited(&mut events, Duration::from_secs(10)).await;
}

#[tokio::test]
async fn a_hung_set_mode_reports_the_steps_before_it_and_the_agents_stderr() {
    let dir = tmp();
    let mut h = fake_harness(json!({"hang_at": "session/set_mode"}));
    h.startup_timeout = Duration::from_millis(500);
    h.mode_after_new = Some("bypassPermissions".into());
    let (err, _events) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Timeout { step, stderr, .. } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/set_mode");
    assert!(
        stderr
            .as_deref()
            .is_some_and(|t| t.contains("hanging at session/set_mode")),
        "{stderr:?}"
    );
    let steps = err.startup_steps();
    assert_eq!(
        step_names(steps),
        ["initialize", "session/new", "session/set_mode"]
    );
    // The two that answered are quick; the last one waited out the timeout.
    assert!(steps[0].millis < 500 && steps[1].millis < 500, "{steps:?}");
    assert!(steps[2].millis >= 500, "{steps:?}");
}

#[tokio::test]
async fn a_started_agent_reports_how_long_each_startup_step_took() {
    let dir = tmp();
    // The test harness also sets a mode and a model: one step each.
    let h = fake_harness(json!({}));
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("expected Ready")
    };
    assert_eq!(
        step_names(&info.startup_steps),
        [
            "initialize",
            "session/new",
            "session/set_mode",
            "session/set_config_option"
        ]
    );
    assert_eq!(s.handle.info().startup_steps, info.startup_steps);
    s.handle.shutdown().await;

    // Without a mode or model to set, only the steps that open the session.
    let mut plain = HarnessConfig::plain(h.command.clone(), vec![]);
    plain.env = h.env.clone();
    let s = start(&plain, dir.path(), SpawnOptions::default()).await;
    assert_eq!(
        step_names(&s.handle.info().startup_steps),
        ["initialize", "session/new"]
    );
    s.handle.shutdown().await;

    // A restored session is `session/load` (and is already in its mode).
    let mut s = start(&plain, dir.path(), resume("prev-123")).await;
    let AgentEvent::Output(_) = s.next().await else {
        panic!("history replay must precede Ready")
    };
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("expected Ready")
    };
    assert_eq!(
        step_names(&info.startup_steps),
        ["initialize", "session/load"]
    );
    s.handle.shutdown().await;
}

#[tokio::test]
async fn a_failed_startup_step_is_the_last_one_reported() {
    let dir = tmp();
    let h = fake_harness(json!({"fail_at": "session/new"}));
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    assert_eq!(
        step_names(err.startup_steps()),
        ["initialize", "session/new"]
    );
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

/// What the agent was asked to change at startup (`session/set_mode` and
/// `session/set_config_option` received), as the fake reports it.
async fn startup_writes(h: &HarnessConfig, options: SpawnOptions) -> String {
    let mut s = start(h, tmp().path(), options).await;
    s.handle.prompt_text("go").expect("prompt");
    let (events, _) = s.turn().await;
    s.handle.shutdown().await;
    message_text(&events)
}

#[tokio::test]
async fn a_mode_and_model_the_agent_already_has_are_not_set_again() {
    let report = json!({"turns": [{"actions": ["report_writes"]}, {"actions": ["report_writes"]}]});
    let with = |extra: serde_json::Value| {
        let mut script = report.clone();
        script
            .as_object_mut()
            .expect("object")
            .extend(extra.as_object().expect("object").clone());
        script
    };

    // The agent starts in another mode on another model: both are set.
    let h = fake_harness(report.clone());
    assert_eq!(
        startup_writes(&h, SpawnOptions::default()).await,
        "writes:modes=bypassPermissions;options=model"
    );

    // It already is in the wanted mode and on the wanted model: nothing is
    // sent, for a new session and for a restored one alike.
    let h = fake_harness(with(
        json!({"modes": ["bypassPermissions", "default"], "models": ["haiku", "default"]}),
    ));
    assert_eq!(
        startup_writes(&h, SpawnOptions::default()).await,
        "writes:modes=;options="
    );
    assert_eq!(
        startup_writes(&h, resume("prev-123")).await,
        "writes:modes=;options="
    );

    // Only what differs is set.
    let h = fake_harness(with(json!({"modes": ["bypassPermissions", "default"]})));
    assert_eq!(
        startup_writes(&h, SpawnOptions::default()).await,
        "writes:modes=;options=model"
    );
}

#[tokio::test]
async fn a_mode_reported_only_as_a_config_option_is_not_set_again() {
    // OpenCode reports its mode as the `mode` option, with no `modes` state.
    let script = |modes: serde_json::Value| {
        json!({
            "modes": modes,
            "modes_as_option": true,
            "models": ["haiku", "default"],
            "turns": [{"actions": ["report_writes"]}, {"actions": ["report_writes"]}]
        })
    };

    // Already in the wanted mode: no `session/set_mode`, for a new session and
    // for a restored one alike.
    let h = fake_harness(script(json!(["bypassPermissions", "default"])));
    assert_eq!(
        startup_writes(&h, SpawnOptions::default()).await,
        "writes:modes=;options="
    );
    assert_eq!(
        startup_writes(&h, resume("prev-123")).await,
        "writes:modes=;options="
    );

    // In another mode: it is still set.
    let h = fake_harness(script(json!(["default", "bypassPermissions"])));
    assert_eq!(
        startup_writes(&h, SpawnOptions::default()).await,
        "writes:modes=bypassPermissions;options="
    );
}

#[tokio::test]
async fn an_effort_the_agent_already_has_is_not_set_again() {
    let script = json!({
        "models": ["haiku", "default"],
        "efforts": {"haiku": ["low", "high"]},
        "turns": [{"actions": ["report_writes"]}]
    });
    let effort = |value: &str| ModelSelect {
        config_id: "effort".into(),
        value: value.into(),
    };
    let mut h = fake_harness(script);
    h.effort = Some(effort("default"));
    assert_eq!(
        startup_writes(&h, SpawnOptions::default()).await,
        "writes:modes=bypassPermissions;options="
    );
    h.effort = Some(effort("high"));
    assert_eq!(
        startup_writes(&h, SpawnOptions::default()).await,
        "writes:modes=bypassPermissions;options=effort"
    );
}

#[tokio::test]
async fn events_after_startup_failure_end_with_exited() {
    let dir = tmp();
    let h = fake_harness(json!({"fail_at": "initialize"}));
    let (_, mut events) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let ev = next_event(&mut events, Duration::from_secs(5)).await;
    assert!(matches!(ev, AgentEvent::Exited { .. }), "{ev:?}");
}
