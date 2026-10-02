//! Fake-agent tests for MiniMax Code's ACP server (`mcode acp`): the
//! MiniMax Code preset driving the fake agent scripted like `mcode` (a
//! `permissionMode` option that must stay untouched, models in the
//! `m:minimax:<Model>:v:<variant>` form, the `thinkingEffort` option that only
//! some models have, hyphenated permission option ids, an authentication
//! error). `mcode` itself is billed per use and not available to automated
//! tests, so this is the only automatic check of MiniMax Code support.

mod common;

use std::sync::Arc;

use common::minimax_code::{
    FLASH_WITHOUT_THINKING, M3_THINKING, fake_minimax_code_harness, fake_minimax_code_preset,
    minimax_code_script,
};
use common::{message_text, start, start_err};
use serde_json::json;
use yhtye_core::acp::schema::{PermissionOptionKind, StopReason};
use yhtye_core::acp::{AgentError, AgentEvent, MINIMAX_CODE_DEFAULT_MODEL, SpawnOptions};
use yhtye_core::agents::ModelService;
use yhtye_core::secrets::Secrets;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn effort_values(model: &yhtye_core::agents::ModelOption) -> Option<Vec<&str>> {
    let efforts = model.efforts.as_ref()?;
    Some(efforts.iter().map(|e| e.value.as_str()).collect())
}

#[tokio::test]
async fn neither_the_mode_nor_the_permission_mode_is_ever_written() {
    let dir = tmp();
    // The agent starts on a model that is not the cheapest; no model is chosen.
    let script = minimax_code_script(json!({"turns": [{"actions": ["report_writes"]}]}));
    let h = fake_minimax_code_harness(script, None, None);
    assert_eq!(h.mode_after_new, None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;

    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    // `permissionMode` and the mode would be saved to the user's own settings.
    assert_eq!(info.current_mode(), Some("default"));
    assert_eq!(info.config_value("permissionMode"), Some("auto"));
    // Only the model: the preset's own default replaces the agent's.
    assert_eq!(info.config_value("model"), Some(MINIMAX_CODE_DEFAULT_MODEL));

    s.handle.prompt_text("go").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert_eq!(message_text(&events), "writes:modes=;options=model");
    s.handle.shutdown().await;
}

#[tokio::test]
async fn the_role_prompt_rides_the_first_prompt_only_and_nothing_is_added_to_it() {
    let dir = tmp();
    let script = minimax_code_script(
        json!({"turns": [{"actions": ["report_state"]}, {"actions": ["report_state"]}]}),
    );
    let h = fake_minimax_code_harness(script, None, None);
    let opts = SpawnOptions {
        system_prompt: Some("ROLE".into()),
        ..SpawnOptions::default()
    };
    let mut s = start(&h, dir.path(), opts).await;
    s.next().await;

    s.handle.prompt_text("one").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    // `_meta.systemPrompt` is ignored by `mcode`: the prompt is prepended to the
    // first prompt, without a note about its MCP tools (they keep their names).
    assert_eq!(
        message_text(&events),
        format!(
            "state:mode=default;model={MINIMAX_CODE_DEFAULT_MODEL};system_prompt=<none>;effort=default;prompt=ROLE|one"
        )
    );
    s.handle.prompt_text("two").expect("prompt");
    let (events, _) = s.turn().await;
    assert!(message_text(&events).ends_with(";prompt=two"));
    s.handle.shutdown().await;
}

#[tokio::test]
async fn the_effort_is_set_on_thinking_effort_after_the_model() {
    let dir = tmp();
    let script = minimax_code_script(json!({"turns": [{"actions": ["report_writes"]}]}));

    let h = fake_minimax_code_harness(
        script.clone(),
        Some(MINIMAX_CODE_DEFAULT_MODEL),
        Some("low"),
    );
    assert_eq!(
        h.effort.as_ref().map(|e| e.config_id.as_str()),
        Some("thinkingEffort")
    );
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    let AgentEvent::Ready(info) = s.next().await else {
        panic!("expected Ready")
    };
    assert_eq!(info.config_value("thinkingEffort"), Some("low"));
    s.handle.prompt_text("go").expect("prompt");
    let (events, _) = s.turn().await;
    // The option exists only once the model is selected: model, then effort.
    assert_eq!(
        message_text(&events),
        "writes:modes=;options=model,thinkingEffort"
    );

    // Another model has no such option any more.
    let options = s
        .handle
        .set_config_option("model", M3_THINKING)
        .await
        .expect("switch the model");
    assert_eq!(
        yhtye_core::acp::config_value(&options, "thinkingEffort"),
        None
    );
    s.handle.shutdown().await;

    // An effort for a model that has none fails the start visibly.
    let h = fake_minimax_code_harness(script, Some(M3_THINKING), Some("low"));
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
async fn permissions_pick_the_hyphenated_allow_once_and_never_allow_always() {
    let dir = tmp();
    // The options `mcode` offers, ids in the hyphenated form.
    let script = minimax_code_script(json!({
        "permission_options": [
            choice("allow-always", "allow_always"),
            choice("deny", "reject_once"),
            choice("allow-once", "allow_once"),
        ],
        "turns": [
            {"actions": ["ask_permission"]},
            {"actions": [{"request_permission": [
                choice("allow-always", "allow_always"),
                choice("deny", "reject_once"),
            ]}]},
        ]
    }));
    let h = fake_minimax_code_harness(script, None, None);
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;

    let expected = [
        (
            Some(PermissionOptionKind::AllowOnce),
            "permission:selected:allow-once",
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
async fn an_authentication_error_reads_as_a_startup_error() {
    let dir = tmp();
    let script = minimax_code_script(json!({
        "fail_at": "session/new",
        "fail_kind": "auth_required",
        "fail_message": "Authentication required: Run `mcode login` and try again.",
    }));
    let h = fake_minimax_code_harness(script, None, None);
    let (err, mut events) = start_err(&h, dir.path(), SpawnOptions::default()).await;

    let AgentError::Startup { step, message } = &err else {
        panic!("{err:?}")
    };
    assert_eq!(step, "session/new");
    assert!(message.contains("mcode login"), "{message}");
    assert!(err.to_string().contains("session/new"), "{err}");
    assert!(matches!(
        common::next_event(&mut events, common::EVENT_TIMEOUT).await,
        AgentEvent::Exited { .. }
    ));
}

#[tokio::test]
async fn the_listing_leaves_out_the_model_that_mcode_refuses_and_reads_the_efforts_of_the_rest() {
    let dir = tmp();
    let preset = fake_minimax_code_preset(minimax_code_script(json!({"turns": []})));
    let service = Arc::new(ModelService::new(
        dir.path().join("probe"),
        Arc::new(Secrets::none()),
    ));
    let listed = service.get(&preset, false).await.expect("listing");

    let values: Vec<&str> = listed.models.iter().map(|m| m.value.as_str()).collect();
    assert_eq!(
        values,
        [
            M3_THINKING,
            MINIMAX_CODE_DEFAULT_MODEL,
            "m:minimax:MiniMax-M2.7:v:thinking"
        ]
    );
    assert!(!values.contains(&FLASH_WITHOUT_THINKING));
    let flash = &listed.models[1];
    assert_eq!(
        effort_values(flash),
        Some(vec!["low", "medium", "high", "xhigh", "max"])
    );
    assert_eq!(effort_values(&listed.models[0]), Some(vec![]));
    service.close().await;
}
