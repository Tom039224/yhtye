//! Fake-agent tests for pi through `@openma/pi-acp`: the pi preset driving the
//! fake agent scripted like the adapter (no modes, the model grouped by provider,
//! the starting model and thinking level read from `PI_ACP_MODEL`, a `thinking`
//! option only for the models that reason, the boolean option `auto_compaction`,
//! `_session/steering`, permission option ids `yes` / `no` / `choice-<n>`, an
//! authentication error). pi needs the user's providers and credentials and
//! `set_config_option` would be saved to their `~/.pi/agent/settings.json`, so
//! this is the only automatic check of pi support.

mod common;

use std::sync::Arc;

use agent_client_protocol::schema::v1::SessionId;
use common::pi::{
    DEEPSEEK, GPT, HAIKU, HAIKU_LEVELS, NEMOTRON, NEMOTRON_LEVELS, fake_pi_harness, fake_pi_preset,
    pi_script,
};
use common::{message_text, start, start_err};
use serde_json::json;
use yhtye_core::acp::schema::{PermissionOptionKind, StopReason};
use yhtye_core::acp::{AgentError, AgentEvent, PI_LOGIN_HINT, PI_MCP_NOTE, SpawnOptions};
use yhtye_core::agents::{AgentChoice, ModelService, RoleSettings};
use yhtye_core::secrets::Secrets;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn values(efforts: &[yhtye_core::agents::EffortOption]) -> Vec<&str> {
    efforts.iter().map(|e| e.value.as_str()).collect()
}

#[tokio::test]
async fn the_model_and_the_level_travel_in_the_environment_and_nothing_is_written() {
    let dir = tmp();
    let script = pi_script(json!({"turns": [
        {"actions": [{"report_env": ["PI_ACP_MODEL", "PI_ACP_QUIET_STARTUP"]}, "report_writes", "report_state"]},
    ]}));

    let h = fake_pi_harness(script.clone(), Some(HAIKU), Some("low"));
    assert_eq!(h.mode_after_new, None);
    assert!(h.model.is_none() && h.effort.is_none());
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    // pi has no modes: none is reported and none is asked for (the fake
    // refuses every `session/set_mode`).
    assert!(info.modes.is_none());
    assert_eq!(info.current_mode(), None);
    assert_eq!(info.config_value("model"), Some(HAIKU));
    assert_eq!(info.config_value("thinking"), Some("low"));
    s.handle.prompt_text("go").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert_eq!(
        message_text(&events),
        format!(
            "env:PI_ACP_MODEL={HAIKU}:low;PI_ACP_QUIET_STARTUP=1\
             writes:modes=;options=\
             state:mode=;model={HAIKU};system_prompt=<none>;effort=low;prompt=go"
        ),
        "neither set_mode nor set_config_option reached pi: they would be saved in its settings"
    );
    s.handle.shutdown().await;

    // A model id with a colon of its own, and no level chosen: nothing follows the id.
    let script = pi_script(json!({"turns": [
        {"actions": [{"report_env": ["PI_ACP_MODEL"]}, "report_state"]},
    ]}));
    let h = fake_pi_harness(script.clone(), Some(NEMOTRON), None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    assert_eq!(info.config_value("model"), Some(NEMOTRON));
    assert_eq!(info.config_value("thinking"), Some(NEMOTRON_LEVELS[0]));
    s.handle.prompt_text("go").expect("prompt");
    let (events, _) = s.turn().await;
    assert!(
        message_text(&events).starts_with(&format!("env:PI_ACP_MODEL={NEMOTRON}state:")),
        "{}",
        message_text(&events)
    );
    s.handle.shutdown().await;

    // A model that does not reason has no `thinking` option; `auto_compaction`
    // (a boolean) is no effort.
    let h = fake_pi_harness(script.clone(), Some(DEEPSEEK), None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    assert_eq!(info.config_value("model"), Some(DEEPSEEK));
    assert_eq!(info.config_value("thinking"), None);
    assert!(
        info.config_options
            .iter()
            .any(|o| &*o.id.0 == "auto_compaction"),
        "the boolean option is there, and left alone"
    );
    s.handle.shutdown().await;

    // Without a model nothing is passed: pi's own default applies (the first of
    // the fake's list). The settings refuse this for pi (it requires a model).
    let h = fake_pi_harness(script, None, None);
    assert!(!h.env.contains_key("PI_ACP_MODEL"));
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    assert_eq!(info.config_value("model"), Some(HAIKU));
    s.handle.shutdown().await;
}

#[tokio::test]
async fn a_model_pi_does_not_list_is_run_as_it_is_and_never_replaced_by_another() {
    let dir = tmp();
    // pi's adapter builds a model out of any id of a known provider: the
    // session reports exactly the model that was asked for (a prompt then fails
    // at the provider), never pi's own, priciest, default.
    let h = fake_pi_harness(pi_script(json!({})), Some("orcarouter/nobody/gone"), None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    assert_eq!(info.config_value("model"), Some("orcarouter/nobody/gone"));
    assert_eq!(info.config_value("thinking"), None);
    s.handle.shutdown().await;
}

#[tokio::test]
async fn the_role_prompt_rides_the_first_prompt_with_the_note_and_yhtye_keeps_its_own_name() {
    let dir = tmp();
    let script = pi_script(json!({"turns": [
        {"actions": ["report_meta", "report_state", "report_client_info"]},
        {"actions": ["report_state"]},
    ]}));
    let h = fake_pi_harness(script, Some(DEEPSEEK), None);
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
    // The adapter ignores `_meta.systemPrompt`: nothing is sent there, and the
    // role prompt is prepended to the first prompt, followed by the note about
    // loading Yhtye's deferred MCP tools.
    let text = message_text(&events);
    assert!(text.starts_with("meta:{}"), "{text}");
    assert!(
        text.contains(&format!(";prompt=ROLE\n\n{PI_MCP_NOTE}|one")),
        "{text}"
    );
    assert!(text.contains("system_prompt=<none>"), "{text}");
    assert!(
        text.contains("client_info:name=yhtye;"),
        "no other client's name is claimed: {text}"
    );
    s.handle.prompt_text("two").expect("prompt");
    let (events, _) = s.turn().await;
    assert!(message_text(&events).ends_with(";prompt=two"));
    s.handle.shutdown().await;
}

fn choice(id: &str, kind: &str) -> serde_json::Value {
    json!({"id": id, "kind": kind})
}

#[tokio::test]
async fn permissions_of_extension_dialogs_pick_the_allow_once_option() {
    let dir = tmp();
    // The adapter's options for an extension's dialogs: a confirm offers
    // `yes` / `no`, a select offers `choice-<n>` (all `allow_once`).
    let script = pi_script(json!({
        "permission_options": [choice("no", "reject_once"), choice("yes", "allow_once")],
        "turns": [
            {"actions": ["ask_permission"]},
            {"actions": [{"request_permission": [
                choice("choice-0", "allow_once"),
                choice("choice-1", "allow_once"),
                choice("choice-2", "allow_once"),
            ]}]},
            {"actions": [{"request_permission": [choice("no", "reject_once")]}]},
        ]
    }));
    let h = fake_pi_harness(script, Some(HAIKU), None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;

    let expected = [
        (
            Some(PermissionOptionKind::AllowOnce),
            "permission:selected:yes",
            2,
        ),
        (
            Some(PermissionOptionKind::AllowOnce),
            "permission:selected:choice-0",
            3,
        ),
        (None, "permission:cancelled", 1),
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
async fn an_authentication_error_says_how_to_log_in_when_opening_or_loading_a_session() {
    let dir = tmp();
    let adapter_message = "Authentication required: no model is available: no API key found";
    let script = pi_script(json!({
        "fail_at": "session/new",
        "fail_kind": "auth_required",
        "fail_message": adapter_message,
    }));
    let h = fake_pi_harness(script, Some(HAIKU), None);
    let (err, mut events) = start_err(&h, dir.path(), SpawnOptions::default()).await;

    let AgentError::Startup { step, message } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/new");
    assert!(
        message.contains(adapter_message),
        "the adapter's own words stay: {message}"
    );
    assert!(message.contains(PI_LOGIN_HINT), "{message}");
    assert!(matches!(
        common::next_event(&mut events, common::EVENT_TIMEOUT).await,
        AgentEvent::Exited { .. }
    ));

    // The same error when a session is restored.
    let script = pi_script(json!({
        "fail_at": "session/load",
        "fail_kind": "auth_required",
        "fail_message": adapter_message,
    }));
    let h = fake_pi_harness(script, Some(HAIKU), None);
    let opts = SpawnOptions {
        resume: Some(SessionId::new("prev-1")),
        ..SpawnOptions::default()
    };
    let (err, _) = start_err(&h, dir.path(), opts).await;
    let AgentError::Startup { step, message } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/load");
    assert!(message.contains(PI_LOGIN_HINT), "{message}");

    // Only an authentication error gets the hint.
    let script = pi_script(json!({"fail_at": "session/new"}));
    let h = fake_pi_harness(script, Some(HAIKU), None);
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Startup { message, .. } = &err else {
        panic!("{err:?}")
    };
    assert!(!message.contains("/login"), "{message}");
}

#[tokio::test]
async fn steering_is_read_from_initialize_and_a_steer_joins_the_running_turn() {
    use yhtye_core::acp::SteerOutcome;

    let dir = tmp();
    let script = pi_script(json!({"turns": [
        {"actions": [{"message": "working"}, "wait_cancel"]},
    ]}));
    let h = fake_pi_harness(script, Some(DEEPSEEK), None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    assert!(info.supports_steering);

    s.handle.prompt_text("work").expect("prompt");
    s.until_message("working").await;
    assert_eq!(
        s.handle.steer_text("change course").await,
        Ok(SteerOutcome::Injected)
    );
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert_eq!(message_text(&events), "steered:change course");
    s.handle.shutdown().await;
}

#[tokio::test]
async fn the_listing_reads_the_models_and_asks_pi_for_no_effort_up_front() {
    let dir = tmp();
    // Selecting anything would fail: the listing must never select (pi saves it
    // to the user's settings).
    let preset = fake_pi_preset(pi_script(json!({
        "fail_at": "session/set_config_option",
        "turns": [],
    })));
    let service = Arc::new(ModelService::new(
        dir.path().join("probe"),
        Arc::new(Secrets::none()),
    ));
    let listed = service.get(&preset, false).await.expect("listing");

    // The groups of the model option are flattened, in the adapter's order.
    let models: Vec<&str> = listed.models.iter().map(|m| m.value.as_str()).collect();
    assert_eq!(models, [HAIKU, DEEPSEEK, NEMOTRON, GPT]);
    assert_eq!(listed.current.as_deref(), Some(HAIKU));
    assert!(
        listed.models.iter().all(|m| m.efforts.is_none()),
        "the efforts are read per model, on demand: {listed:?}"
    );
    assert_eq!(service.cached_efforts("pi", HAIKU), None);
    service.close().await;
}

#[tokio::test]
async fn the_efforts_of_a_model_are_those_of_a_session_started_on_it() {
    let dir = tmp();
    let preset = fake_pi_preset(pi_script(json!({
        "fail_at": "session/set_config_option",
        "turns": [],
    })));
    let service = Arc::new(ModelService::new(
        dir.path().join("probe"),
        Arc::new(Secrets::none()),
    ));

    // The levels depend on the model, and `auto_compaction` is none of them.
    let efforts = service.get_efforts(&preset, HAIKU).await.expect("haiku");
    assert_eq!(values(&efforts), HAIKU_LEVELS);
    let efforts = service
        .get_efforts(&preset, NEMOTRON)
        .await
        .expect("nemotron");
    assert_eq!(values(&efforts), NEMOTRON_LEVELS);
    assert!(
        service
            .get_efforts(&preset, DEEPSEEK)
            .await
            .expect("deepseek")
            .is_empty(),
        "a model that does not reason has no effort"
    );
    assert!(
        service
            .get_efforts(&preset, GPT)
            .await
            .expect("gpt")
            .is_empty()
    );

    // What was read is kept, and it is what a stored effort is checked against.
    let cached = service.cached_efforts("pi", HAIKU).expect("cached");
    assert_eq!(values(&cached), HAIKU_LEVELS);
    let check =
        |model: &str, effort: &str| {
            RoleSettings::only(AgentChoice::new("pi", Some(model)).with_effort(effort))
                .check_efforts(&[], |harness, model| {
                    let efforts = service.cached_efforts(harness, model)?;
                    Some(efforts.into_iter().map(|e| e.value).collect())
                })
        };
    assert!(check(HAIKU, "xhigh").is_ok());
    assert!(check(NEMOTRON, "minimal").is_ok());
    assert!(check(HAIKU, "minimal").is_err(), "that is nemotron's level");
    assert!(check(DEEPSEEK, "low").is_err(), "this model has no effort");

    // A model pi does not list has no effort to offer.
    let efforts = service
        .get_efforts(&preset, "orcarouter/nobody/gone")
        .await
        .expect("unknown model");
    assert!(efforts.is_empty());
    service.close().await;
}
