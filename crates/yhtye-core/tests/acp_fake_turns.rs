//! Fake-agent tests: handshake, streaming, cancel, permissions, typed updates.

mod common;

use std::time::{Duration, Instant};

use common::{fake_harness, message_text, start};
use serde_json::json;
use yhtye_core::acp::schema::{PermissionOptionKind, StopReason};
use yhtye_core::acp::{AgentError, AgentEvent, AgentOutput, SpawnOptions, SystemPromptStyle};

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

#[tokio::test]
async fn handshake_applies_mode_model_and_system_prompt() {
    let dir = tmp();
    let h = fake_harness(json!({"turns": [{"actions": ["report_state"]}]}));
    let opts = SpawnOptions {
        system_prompt: Some("ROLE-PROMPT".into()),
        ..SpawnOptions::default()
    };
    let mut s = start(&h, dir.path(), opts).await;

    let AgentEvent::Ready(info) = s.next().await else {
        panic!("first event must be Ready")
    };
    assert_eq!(info.current_mode(), Some("bypassPermissions"));
    assert_eq!(info.config_value("model"), Some("haiku"));
    assert!(!info.resumed);
    assert!(info.capabilities.load_session);
    assert_eq!(s.handle.info().acp_session_id, info.acp_session_id);

    s.handle.prompt_text("hello").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    assert_eq!(
        message_text(&events),
        "state:mode=bypassPermissions;model=haiku;system_prompt=ROLE-PROMPT;prompt=hello"
    );
    s.handle.shutdown().await;
}

#[tokio::test]
async fn first_prompt_style_prepends_system_prompt_once() {
    let dir = tmp();
    let mut h = fake_harness(
        json!({"turns": [{"actions": ["report_state"]}, {"actions": ["report_state"]}]}),
    );
    h.system_prompt = SystemPromptStyle::FirstPrompt;
    let opts = SpawnOptions {
        system_prompt: Some("SYS".into()),
        ..SpawnOptions::default()
    };
    let mut s = start(&h, dir.path(), opts).await;
    s.next().await; // Ready

    s.handle.prompt_text("one").expect("prompt");
    let (events, _) = s.turn().await;
    assert!(message_text(&events).ends_with("system_prompt=<none>;prompt=SYS|one"));
    s.handle.prompt_text("two").expect("prompt");
    let (events, _) = s.turn().await;
    assert!(message_text(&events).ends_with("prompt=two"));
    s.handle.shutdown().await;
}

#[tokio::test]
async fn chunks_stream_in_order_and_stop_reasons_are_surfaced() {
    let dir = tmp();
    let h = fake_harness(json!({"turns": [
        {"match": "stream", "actions": [
            {"thought": "thinking"}, {"message": "a"}, {"sleep": 20}, {"message": "b"}, {"message": "c"}
        ]},
        {"match": "limit", "actions": [{"message": "x"}, {"end": "max_tokens"}]},
        {"match": "refuse", "actions": [{"end": "refusal"}]}
    ]}));
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;

    s.handle.prompt_text("stream please").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));
    let texts: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Output(o) => o.chunk_text().map(str::to_string),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["thinking", "a", "b", "c"]);
    assert!(matches!(
        events[0],
        AgentEvent::Output(AgentOutput::ThoughtChunk(_))
    ));

    s.handle.prompt_text("limit").expect("prompt");
    assert_eq!(s.turn().await.1, Ok(StopReason::MaxTokens));
    s.handle.prompt_text("refuse").expect("prompt");
    assert_eq!(s.turn().await.1, Ok(StopReason::Refusal));
    s.handle.shutdown().await;
}

#[tokio::test]
async fn cancel_mid_turn_ends_with_cancelled_and_session_stays_usable() {
    let dir = tmp();
    let h = fake_harness(json!({"turns": [
        {"match": "long", "actions": [{"message": "started"}, {"sleep": 60000}, {"message": "never"}]},
        {"match": "wait", "actions": [{"message": "waiting"}, "wait_cancel"]},
        {"match": "again", "actions": [{"message": "ok"}]}
    ]}));
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;

    s.handle.prompt_text("long").expect("prompt");
    s.until_message("started").await;
    assert!(s.handle.is_turn_running());
    assert_eq!(s.handle.prompt_text("second"), Err(AgentError::Busy));
    let t0 = Instant::now();
    s.handle.cancel().expect("cancel");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::Cancelled));
    assert!(
        t0.elapsed() < Duration::from_secs(5),
        "cancel took {:?}",
        t0.elapsed()
    );
    assert!(!message_text(&events).contains("never"));

    s.handle.prompt_text("wait").expect("prompt");
    s.until_message("waiting").await;
    s.handle.cancel().expect("cancel");
    assert_eq!(s.turn().await.1, Ok(StopReason::Cancelled));

    s.handle.prompt_text("again").expect("prompt after cancel");
    let (events, stop) = s.turn().await;
    assert_eq!(
        (message_text(&events).as_str(), stop),
        ("ok", Ok(StopReason::EndTurn))
    );
    s.handle.shutdown().await;
}

fn perm(options: &[(&str, &str)]) -> serde_json::Value {
    let options: Vec<_> = options
        .iter()
        .map(|(id, kind)| json!({"id": id, "kind": kind}))
        .collect();
    json!({"request_permission": options})
}

#[tokio::test]
async fn permission_is_auto_answered_by_kind_not_position() {
    let dir = tmp();
    let h = fake_harness(json!({"turns": [
        {"actions": [perm(&[("r1", "reject_once"), ("o1", "allow_once"), ("a1", "allow_always")])]},
        {"actions": [perm(&[("r2", "reject_always"), ("r3", "reject_once"), ("o2", "allow_once")])]},
        {"actions": [perm(&[("r4", "reject_once"), ("r5", "reject_always")])]}
    ]}));
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;

    let expected = [
        (
            Some(PermissionOptionKind::AllowAlways),
            "permission:selected:a1",
        ),
        (
            Some(PermissionOptionKind::AllowOnce),
            "permission:selected:o2",
        ),
        (None, "permission:cancelled"),
    ];
    for (want_kind, want_text) in expected {
        s.handle.prompt_text("go").expect("prompt");
        let (events, stop) = s.turn().await;
        assert_eq!(stop, Ok(StopReason::EndTurn));
        let chosen = events.iter().find_map(|e| match e {
            AgentEvent::PermissionAutoAnswered {
                chosen, options, ..
            } => {
                assert!(options.len() >= 2);
                Some(*chosen)
            }
            _ => None,
        });
        assert_eq!(chosen, Some(want_kind));
        assert_eq!(message_text(&events), want_text);
    }
    s.handle.shutdown().await;
}

#[tokio::test]
async fn every_session_update_kind_is_typed_and_unknown_is_kept() {
    let dir = tmp();
    let text = json!({"type": "text", "text": "u"});
    let h = fake_harness(json!({"turns": [{"actions": [
        {"update": {"sessionUpdate": "user_message_chunk", "content": text}},
        {"message": "m"},
        {"thought": "t"},
        {"tool_call": {"id": "tc1", "title": "Edit file"}},
        {"tool_call_update": {"id": "tc1", "status": "completed"}},
        {"plan": ["step one"]},
        {"update": {"sessionUpdate": "available_commands_update", "availableCommands": []}},
        {"update": {"sessionUpdate": "current_mode_update", "currentModeId": "default"}},
        {"update": {"sessionUpdate": "config_option_update", "configOptions": []}},
        {"update": {"sessionUpdate": "session_info_update", "title": "T"}},
        {"update": {"sessionUpdate": "usage_update", "used": 10, "size": 100}},
        {"update": {"sessionUpdate": "brand_new_kind", "x": 1}}
    ]}]}));
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;
    s.handle.prompt_text("all").expect("prompt");
    let (events, stop) = s.turn().await;
    assert_eq!(stop, Ok(StopReason::EndTurn));

    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            AgentEvent::Output(o) => match o {
                AgentOutput::UserMessageChunk(_) => "user",
                AgentOutput::MessageChunk(_) => "message",
                AgentOutput::ThoughtChunk(_) => "thought",
                AgentOutput::ToolCall(t) => {
                    assert_eq!(&*t.tool_call_id.0, "tc1");
                    "tool_call"
                }
                AgentOutput::ToolCallUpdate(_) => "tool_call_update",
                AgentOutput::Plan(p) => {
                    assert_eq!(p.entries[0].content, "step one");
                    "plan"
                }
                AgentOutput::AvailableCommands(_) => "commands",
                AgentOutput::ModeChanged(m) => {
                    assert_eq!(&*m.current_mode_id.0, "default");
                    "mode"
                }
                AgentOutput::ConfigOptions(_) => "config",
                AgentOutput::SessionInfo(_) => "info",
                AgentOutput::Usage(u) => {
                    assert_eq!((u.used, u.size), (10, 100));
                    "usage"
                }
                AgentOutput::Unknown(v) => {
                    assert_eq!(v["sessionUpdate"], "brand_new_kind");
                    "unknown"
                }
            },
            other => panic!("unexpected event {other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "user",
            "message",
            "thought",
            "tool_call",
            "tool_call_update",
            "plan",
            "commands",
            "mode",
            "config",
            "info",
            "usage",
            "unknown"
        ]
    );
    // Events serialize (they are forwarded to the UI later).
    serde_json::to_string(&events).expect("events serialize");
    s.handle.shutdown().await;
}

#[tokio::test]
async fn set_mode_and_config_option_at_runtime() {
    let dir = tmp();
    let h = fake_harness(
        json!({"models": ["default", "haiku", "sonnet"], "turns": [{"actions": ["report_state"]}]}),
    );
    let mut s = start(&h, dir.path(), SpawnOptions::default()).await;
    s.next().await;

    s.handle.set_mode("default").await.expect("set_mode");
    let options = s
        .handle
        .set_config_option("model", "sonnet")
        .await
        .expect("set model");
    assert_eq!(
        yhtye_core::acp::config_value(&options, "model"),
        Some("sonnet")
    );
    let err = s
        .handle
        .set_config_option("model", "gpt")
        .await
        .expect_err("unknown model");
    assert!(matches!(err, AgentError::Request { .. }), "{err:?}");

    s.handle.prompt_text("x").expect("prompt");
    let (events, _) = s.turn().await;
    assert!(message_text(&events).starts_with("state:mode=default;model=sonnet;"));
    s.handle.shutdown().await;
}
