//! Fake-agent tests for Devin's ACP server (`devin acp`): the Devin preset
//! driving the fake agent scripted like Devin (bypass mode, a thought-level
//! effort option under another id, `switch_*` / `*_always` permission options,
//! the `_cognition.ai/request_diagnostics` vendor request, an authentication
//! error). Devin itself is not available to automated tests, so this is the
//! only automatic check of Devin support.

mod common;

use common::devin::{BYPASS, devin_script, fake_devin_harness};
use common::{message_text, start, start_err};
use serde_json::json;
use yhtye_core::acp::schema::{PermissionOptionKind, StopReason};
use yhtye_core::acp::{
    AgentError, AgentEvent, ClientInfoOverride, DEVIN_BYPASS_MODE, SpawnOptions,
};

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn report_state_turn() -> serde_json::Value {
    json!({"turns": [{"actions": ["report_state"]}, {"actions": ["report_state"]}]})
}

#[tokio::test]
async fn bypass_mode_is_set_and_the_role_prompt_rides_the_first_prompt() {
    let dir = tmp();
    let h = fake_devin_harness(devin_script(report_state_turn()), None, None);
    let opts = SpawnOptions {
        system_prompt: Some("ROLE".into()),
        ..SpawnOptions::default()
    };
    let mut s = start(&h, dir.path(), opts).await;

    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    assert_eq!(BYPASS, DEVIN_BYPASS_MODE);
    // The fake starts in `default`: `bypass` is there only through `set_mode`.
    assert_eq!(info.current_mode(), Some(BYPASS));

    s.handle.prompt_text("one").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    // No `_meta` hook: the prompt is prepended to the first prompt, once.
    assert_eq!(
        message_text(&events),
        "state:mode=bypass;model=default;system_prompt=<none>;effort=default;prompt=ROLE|one"
    );
    s.handle.prompt_text("two").expect("prompt");
    let (events, _) = s.turn().await;
    assert!(message_text(&events).ends_with(";prompt=two"));
    s.handle.shutdown().await;
}

#[tokio::test]
async fn effort_is_set_on_the_thought_level_option_whatever_its_id() {
    let dir = tmp();
    let script = devin_script(report_state_turn());

    // Yhtye asks for `effort`; this agent calls the option `reasoning`.
    let h = fake_devin_harness(script.clone(), None, Some("high"));
    assert_eq!(
        h.effort.as_ref().map(|e| e.config_id.as_str()),
        Some("effort")
    );
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("expected Ready")
    };
    assert_eq!(info.config_value("reasoning"), Some("high"));
    assert_eq!(info.config_value("effort"), None, "no option of that id");

    s.handle.prompt_text("x").expect("prompt");
    let (events, _) = s.turn().await;
    assert!(message_text(&events).contains(";effort=high;"));

    // The same option keeps working at runtime under its own id.
    let options = s
        .handle
        .set_config_option("reasoning", "low")
        .await
        .expect("set reasoning");
    assert_eq!(
        yhtye_core::acp::config_value(&options, "reasoning"),
        Some("low")
    );
    s.handle.shutdown().await;

    // Without a chosen effort the agent keeps its own.
    let h = fake_devin_harness(script.clone(), None, None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("expected Ready")
    };
    assert_eq!(info.config_value("reasoning"), Some("default"));
    s.handle.shutdown().await;

    // A value the agent does not offer fails the start visibly.
    let h = fake_devin_harness(script, None, Some("max"));
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Startup { step, .. } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/set_config_option");
}

#[tokio::test]
async fn the_client_introduces_itself_as_yhtye_unless_overridden() {
    let dir = tmp();
    let script = devin_script(json!({"turns": [{"actions": ["report_client_info"]}]}));

    let h = fake_devin_harness(script.clone(), None, None);
    assert_eq!(h.client_info, None, "the Devin preset claims no other name");
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;
    s.handle.prompt_text("who").expect("prompt");
    let (events, _) = s.turn().await;
    assert_eq!(
        message_text(&events),
        format!(
            "client_info:name=yhtye;version={}",
            env!("CARGO_PKG_VERSION")
        )
    );
    s.handle.shutdown().await;

    let mut h = fake_devin_harness(script, None, None);
    h.client_info = Some(ClientInfoOverride {
        name: "other-client".into(),
        version: "9.9.9".into(),
    });
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;
    s.handle.prompt_text("who").expect("prompt");
    let (events, _) = s.turn().await;
    assert_eq!(
        message_text(&events),
        "client_info:name=other-client;version=9.9.9"
    );
    s.handle.shutdown().await;
}

#[tokio::test]
async fn diagnostics_requests_are_answered_with_an_empty_object_and_the_session_goes_on() {
    let dir = tmp();
    let script = devin_script(json!({
        "vendor_requests": true,
        "turns": [{"actions": [{"message": "one"}]}, {"actions": [{"message": "two"}]}]
    }));
    let h = fake_devin_harness(script, None, None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;

    for word in ["one", "two"] {
        s.handle.prompt_text("go").expect("prompt");
        let (events, stop) = s.turn().await;
        assert_eq!(stop, Ok(StopReason::EndTurn), "the client must not crash");
        assert_eq!(
            message_text(&events),
            format!("vendor:request_diagnostics:ok:{{}}{word}")
        );
    }
    s.handle.shutdown().await;
}

fn choice(id: &str, kind: &str) -> serde_json::Value {
    json!({"id": id, "kind": kind})
}

#[tokio::test]
async fn permissions_never_pick_mode_switches_plans_or_standing_grants() {
    let dir = tmp();
    // Devin's own options come first; the plain `allow` is last, so choosing by
    // position (or by the first `allow_*` kind) would pick a wrong one.
    let script = devin_script(json!({
        "permission_options": [
            choice("switch_to_accept_edits", "allow_once"),
            choice("plan_approve", "allow_once"),
            choice("allow_command_always", "allow_always"),
            choice("allow_always", "allow_always"),
            choice("reject", "reject_once"),
            choice("allow", "allow_once"),
        ],
        "turns": [
            {"actions": ["ask_permission"]},
            {"actions": [{"request_permission": [
                choice("switch_to_bypass", "allow_once"),
                choice("allow_always", "allow_always"),
                choice("reject", "reject_once"),
            ]}]},
        ]
    }));
    let h = fake_devin_harness(script, None, None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;

    let expected = [
        (
            Some(PermissionOptionKind::AllowOnce),
            "permission:selected:allow",
            6,
        ),
        (None, "permission:cancelled", 3),
    ];
    for (want_kind, want_text, want_options) in expected {
        s.handle.prompt_text("go").expect("prompt");
        let (events, stop) = s.turn().await;
        assert_eq!(stop, Ok(StopReason::EndTurn));
        let answered = events.iter().find_map(|e| match e {
            AgentEvent::PermissionAutoAnswered {
                chosen, options, ..
            } => Some((*chosen, options.len())),
            _ => None,
        });
        assert_eq!(answered, Some((want_kind, want_options)));
        assert_eq!(message_text(&events), want_text);
    }
    s.handle.shutdown().await;
}

#[tokio::test]
async fn an_authentication_error_reads_as_a_startup_error() {
    let dir = tmp();
    let script = devin_script(json!({
        "fail_at": "session/new",
        "fail_kind": "auth_required",
        "fail_message": "not logged in: run `devin auth login`",
    }));
    let h = fake_devin_harness(script, None, None);
    let (err, mut events) = start_err(&h, dir.path(), SpawnOptions::default()).await;

    let AgentError::Startup { step, message } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/new");
    assert!(message.contains("Authentication required"), "{message}");
    assert!(message.contains("devin auth login"), "{message}");
    assert!(err.to_string().contains("session/new"), "{err}");
    assert!(matches!(
        common::next_event(&mut events, common::EVENT_TIMEOUT).await,
        AgentEvent::Exited { .. }
    ));
}
