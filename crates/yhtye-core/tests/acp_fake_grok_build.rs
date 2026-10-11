//! Fake-agent tests for Grok Build's ACP server (`grok agent --no-leader
//! stdio`): the Grok Build preset driving the fake agent scripted like `grok`
//! (no modes, the `reasoning_effort` option, `_x.ai/...` vendor notifications
//! before the `session/new` response and in every turn, `allow_always_*` /
//! `allow_edits_for_session` permission options). `grok` itself needs a
//! grok.com login and uses its quota, so it is not available to automated
//! tests; this is the only automatic check of Grok Build support.

mod common;

use std::sync::Arc;

use common::grok_build::{
    GROK_MODEL, fake_grok_build_harness, fake_grok_build_preset, grok_build_script,
};
use common::{message_text, start, start_err};
use serde_json::json;
use yhtye_core::acp::schema::{PermissionOptionKind, StopReason};
use yhtye_core::acp::{AgentError, AgentEvent, SpawnOptions};
use yhtye_core::agents::ModelService;
use yhtye_core::secrets::Secrets;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

#[tokio::test]
async fn no_mode_is_set_vendor_notifications_are_ignored_and_the_role_prompt_rides_the_first_prompt()
 {
    let dir = tmp();
    let script = grok_build_script(
        json!({"turns": [{"actions": ["report_state"]}, {"actions": ["report_state"]}]}),
    );
    let h = fake_grok_build_harness(script, None, None);
    let opts = SpawnOptions {
        system_prompt: Some("ROLE".into()),
        ..SpawnOptions::default()
    };
    let mut s = start(&h, dir.path(), opts).await;

    // `_x.ai/...` notifications arrived before the `session/new` response.
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    // `grok` has no modes: none is reported and none is asked for (the fake
    // refuses every `session/set_mode`).
    assert!(info.modes.is_none());
    assert_eq!(info.current_mode(), None);
    assert_eq!(info.config_value("model"), Some(GROK_MODEL));

    s.handle.prompt_text("one").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(
        stop,
        Ok(StopReason::EndTurn),
        "vendor notifications are ignored"
    );
    // No `_meta` hook is used: the role prompt is prepended to the first
    // prompt, without a note (`grok` lists MCP tools under their own names).
    assert_eq!(
        message_text(&events),
        format!(
            "state:mode=;model={GROK_MODEL};system_prompt=<none>;effort=default;prompt=ROLE|one"
        )
    );
    s.handle.prompt_text("two").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert!(message_text(&events).ends_with(";prompt=two"));
    s.handle.shutdown().await;
}

#[tokio::test]
async fn the_model_and_then_the_effort_are_set_on_reasoning_effort() {
    let dir = tmp();
    let script = grok_build_script(json!({"turns": [{"actions": ["report_state"]}]}));

    let h = fake_grok_build_harness(script.clone(), Some(GROK_MODEL), Some("low"));
    assert_eq!(
        h.effort.as_ref().map(|e| e.config_id.as_str()),
        Some("reasoning_effort")
    );
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("expected Ready")
    };
    assert_eq!(info.config_value("model"), Some(GROK_MODEL));
    assert_eq!(info.config_value("reasoning_effort"), Some("low"));
    s.handle.prompt_text("x").expect("prompt");
    let (events, _) = s.turn().await;
    assert!(message_text(&events).contains(";effort=low;"));
    s.handle.shutdown().await;

    // A value `grok` does not offer fails the start visibly.
    let h = fake_grok_build_harness(script, None, Some("max"));
    let (err, _) = start_err(&h, dir.path(), SpawnOptions::default()).await;
    let AgentError::Startup { step, .. } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/set_config_option");
}

fn choice(id: &str, kind: &str) -> serde_json::Value {
    json!({"id": id, "kind": kind})
}

#[tokio::test]
async fn permissions_pick_allow_once_and_never_a_standing_or_session_wide_grant() {
    let dir = tmp();
    // `grok`'s option ids. Their kinds are not known from the real agent, so
    // the broader grants are given the `allow_once` kind here: only the id
    // tells them apart, and `allow_once` itself comes last.
    let script = grok_build_script(json!({
        "permission_options": [
            choice("allow_always", "allow_always"),
            choice("allow_edits_for_session", "allow_once"),
            choice("allow_always_bash", "allow_once"),
            choice("allow_always_mcp_server", "allow_once"),
            choice("reject_once", "reject_once"),
            choice("allow_once", "allow_once"),
        ],
        "turns": [
            {"actions": ["ask_permission"]},
            {"actions": [{"request_permission": [
                choice("allow_always", "allow_always"),
                choice("allow_edits_for_session", "allow_once"),
                choice("reject_once", "reject_once"),
            ]}]},
        ]
    }));
    let h = fake_grok_build_harness(script, None, None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;

    let expected = [
        (
            Some(PermissionOptionKind::AllowOnce),
            "permission:selected:allow_once",
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
async fn the_listing_reads_groks_model_and_its_efforts() {
    let dir = tmp();
    let preset = fake_grok_build_preset(grok_build_script(json!({"turns": []})));
    let service = Arc::new(ModelService::new(
        dir.path().join("probe"),
        Arc::new(Secrets::none()),
    ));
    let listed = service.get(&preset, false).await.expect("listing");

    let values: Vec<&str> = listed.models.iter().map(|m| m.value.as_str()).collect();
    assert_eq!(values, [GROK_MODEL]);
    assert_eq!(listed.current.as_deref(), Some(GROK_MODEL));
    let efforts: Option<Vec<&str>> = listed.models[0]
        .efforts
        .as_ref()
        .map(|e| e.iter().map(|e| e.value.as_str()).collect());
    assert_eq!(efforts, Some(vec!["xhigh", "high", "medium", "low"]));
    service.close().await;
}
