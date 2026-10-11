//! Fake-agent tests for Google Antigravity's ACP server (`agy_acp_server`): the
//! Antigravity preset driving the fake agent scripted like the server (modes
//! `default` / `auto_edit` / `yolo` that must stay untouched, models whose
//! thinking level is part of the id, no effort option, permission option ids
//! `allow` / `deny` / `allow_always`, an authentication error). The server needs
//! a Google sign-in and is not available to automated tests, so this is the only
//! automatic check of Antigravity support.

mod common;

use std::sync::Arc;

use common::antigravity::{
    FLASH_HIGH, PRO_HIGH, antigravity_script, fake_antigravity_harness, fake_antigravity_preset,
};
use common::{message_text, start, start_err};
use serde_json::json;
use yhtye_core::acp::schema::{PermissionOptionKind, StopReason};
use yhtye_core::acp::{
    ANTIGRAVITY_DEFAULT_MODEL, ANTIGRAVITY_LOGIN_HINT, AgentError, AgentEvent, SpawnOptions,
};
use yhtye_core::agents::{AgentChoice, AgentRole, ModelService, RoleSettings};
use yhtye_core::secrets::Secrets;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

#[tokio::test]
async fn the_mode_is_never_written_and_the_model_is_the_cheapest_unless_one_is_chosen() {
    let dir = tmp();
    // The agent starts on its own default model, which is not the cheapest.
    let script = antigravity_script(json!({"turns": [{"actions": ["report_writes"]}]}));

    let h = fake_antigravity_harness(script.clone(), None, None);
    assert_eq!(h.mode_after_new, None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    assert_eq!(info.current_mode(), Some("default"));
    assert_eq!(info.config_value("model"), Some(ANTIGRAVITY_DEFAULT_MODEL));
    s.handle.prompt_text("go").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert_eq!(message_text(&events), "writes:modes=;options=model");
    s.handle.shutdown().await;

    // A chosen model replaces the default.
    let h = fake_antigravity_harness(script, Some(PRO_HIGH), None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    assert_eq!(info.config_value("model"), Some(PRO_HIGH));
    s.handle.shutdown().await;
}

#[tokio::test]
async fn the_role_prompt_rides_the_first_prompt_only_and_yhtye_keeps_its_own_name() {
    let dir = tmp();
    let script = antigravity_script(json!({"turns": [
        {"actions": ["report_state", "report_client_info"]},
        {"actions": ["report_state"]},
    ]}));
    let h = fake_antigravity_harness(script, None, None);
    assert_eq!(h.client_info, None);
    let opts = SpawnOptions {
        system_prompt: Some("ROLE".into()),
        ..SpawnOptions::default()
    };
    let mut s = start(&h, dir.path(), opts).await;
    s.next().await;

    s.handle.prompt_text("one").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    // The server has no hook for a system prompt: it is prepended to the first
    // prompt, with nothing added (the MCP tools keep their names).
    let text = message_text(&events);
    assert!(
        text.starts_with(&format!(
            "state:mode=default;model={ANTIGRAVITY_DEFAULT_MODEL};system_prompt=<none>;prompt=ROLE|one"
        )),
        "{text}"
    );
    assert!(
        text.contains("client_info:name=yhtye;"),
        "no editor's name is claimed to unlock models: {text}"
    );
    s.handle.prompt_text("two").expect("prompt");
    let (events, _) = s.turn().await;
    assert!(message_text(&events).ends_with(";prompt=two"));
    s.handle.shutdown().await;
}

#[tokio::test]
async fn an_effort_is_not_sent_because_there_is_no_effort_option() {
    let dir = tmp();
    let script = antigravity_script(json!({"turns": [{"actions": ["report_writes"]}]}));

    // A stored effort (from a settings file edited by hand) never reaches the server.
    let h = fake_antigravity_harness(script, Some(PRO_HIGH), Some("high"));
    assert!(h.effort.is_none());
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;
    s.handle.prompt_text("go").expect("prompt");
    let (events, _) = s.turn().await;
    assert_eq!(message_text(&events), "writes:modes=;options=model");
    s.handle.shutdown().await;
}

fn choice(id: &str, kind: &str) -> serde_json::Value {
    json!({"id": id, "kind": kind})
}

#[tokio::test]
async fn permissions_pick_allow_and_never_allow_always() {
    let dir = tmp();
    // The options the server offers: `allow_always` is saved with the session.
    let script = antigravity_script(json!({
        "permission_options": [
            choice("allow_always", "allow_always"),
            choice("deny", "reject_once"),
            choice("allow", "allow_once"),
        ],
        "turns": [
            {"actions": ["ask_permission"]},
            {"actions": [{"request_permission": [
                choice("allow_always", "allow_always"),
                choice("deny", "reject_once"),
            ]}]},
        ]
    }));
    let h = fake_antigravity_harness(script, None, None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;

    let expected = [
        (
            Some(PermissionOptionKind::AllowOnce),
            "permission:selected:allow",
            3,
        ),
        (None, "permission:cancelled", 2),
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
async fn an_authentication_error_says_how_to_log_in_and_other_errors_do_not() {
    let dir = tmp();
    let server_message = "No authentication method selected. Either call the `authenticate` method";
    let script = antigravity_script(json!({
        "fail_at": "session/new",
        "fail_kind": "auth_required",
        "fail_message": server_message,
    }));
    let h = fake_antigravity_harness(script, None, None);
    let (err, mut events) = start_err(&h, dir.path(), SpawnOptions::default()).await;

    let AgentError::Startup { step, message } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/new");
    assert!(message.contains("Authentication required"), "{message}");
    assert!(
        message.contains(server_message),
        "the server's own words stay: {message}"
    );
    assert!(message.contains(ANTIGRAVITY_LOGIN_HINT), "{message}");
    assert!(err.to_string().contains("session/new"), "{err}");
    assert!(matches!(
        common::next_event(&mut events, common::EVENT_TIMEOUT).await,
        AgentEvent::Exited { .. }
    ));

    // Only an authentication error gets the hint.
    let script = antigravity_script(json!({"fail_at": "session/new"}));
    let h = fake_antigravity_harness(script, None, None);
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Startup { message, .. } = &err else {
        panic!("{err:?}")
    };
    assert!(!message.contains("oauth-personal"), "{message}");

    // The same error from a harness without a hint is shown as the server said it.
    let script = antigravity_script(json!({
        "fail_at": "session/new",
        "fail_kind": "auth_required",
        "fail_message": server_message,
    }));
    let mut h = fake_antigravity_harness(script, None, None);
    h.login_hint = None;
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Startup { message, .. } = &err else {
        panic!("{err:?}")
    };
    assert!(!message.contains("oauth-personal"), "{message}");
}

#[tokio::test]
async fn the_listing_has_no_efforts_and_asks_the_server_for_none() {
    let dir = tmp();
    // Selecting a model would fail: the listing must not need to.
    let preset = fake_antigravity_preset(antigravity_script(json!({
        "fail_at": "session/set_config_option",
        "turns": [],
    })));
    let service = Arc::new(ModelService::new(
        dir.path().join("probe"),
        Arc::new(Secrets::none()),
    ));
    let listed = service.get(&preset, false).await.expect("listing");

    let values: Vec<&str> = listed.models.iter().map(|m| m.value.as_str()).collect();
    assert_eq!(
        values,
        [
            FLASH_HIGH,
            "gemini-3.8-flash-medium",
            ANTIGRAVITY_DEFAULT_MODEL,
            PRO_HIGH,
            "gemini-3.1-pro-low"
        ]
    );
    assert_eq!(listed.current.as_deref(), Some(FLASH_HIGH));
    assert!(
        listed.models.iter().all(|m| m.efforts == Some(Vec::new())),
        "no model has an effort: {listed:?}"
    );

    // So an effort is refused for every model, however it got into the settings.
    let row =
        RoleSettings::only(AgentChoice::new("antigravity", Some(PRO_HIGH)).with_effort("high"));
    let error = row
        .check_efforts(&[], |harness, model| {
            let efforts = service.cached_efforts(harness, model)?;
            Some(efforts.into_iter().map(|e| e.value).collect())
        })
        .expect_err("an effort the model does not offer");
    assert_eq!(
        error,
        format!("antigravity/{PRO_HIGH}: this model has no effort (got high)")
    );
    service.close().await;
}

#[tokio::test]
async fn the_efforts_of_a_model_are_none_without_starting_the_server() {
    let dir = tmp();
    // A server that cannot even be started: asking it would fail.
    let mut preset = fake_antigravity_preset(antigravity_script(json!({"turns": []})));
    preset = preset.with_command("/nonexistent/agy_acp_server.par");
    let service = Arc::new(ModelService::new(
        dir.path().join("probe"),
        Arc::new(Secrets::none()),
    ));
    let efforts = service
        .get_efforts(&preset, PRO_HIGH)
        .await
        .expect("no efforts, and nothing to ask");
    assert!(efforts.is_empty());
    assert!(
        preset
            .config(AgentRole::Orchestrator, Some(PRO_HIGH), Some("high"))
            .effort
            .is_none()
    );
    service.close().await;
}
