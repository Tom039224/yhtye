//! Real Claude Code tests (always Haiku). Ignored by default; run with
//! `cargo test -p yhtye-core --test acp_claude_real -- --ignored --test-threads=1`.
//! Uses the local Claude Code login. Prompts are kept tiny to save tokens.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{Session, assert_group_gone, message_text};
use tokio::sync::mpsc;
use yhtye_core::acp::schema::{PermissionOptionKind, StopReason};
use yhtye_core::acp::{AgentEvent, AgentOutput, HarnessConfig, SpawnOptions, spawn_agent};

const REAL_TIMEOUT: Duration = Duration::from_secs(180);
const MODEL: &str = "haiku";

/// Starts Claude Code on Haiku in bypass mode and asserts both are in effect.
async fn start_claude(cwd: &Path, options: SpawnOptions) -> Session {
    let harness = HarnessConfig::claude_code(MODEL);
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = spawn_agent(&harness, cwd, options, tx)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let info = handle.info();
    assert_eq!(
        info.config_value("model"),
        Some(MODEL),
        "model must be haiku: {:?}",
        info.config_options
    );
    assert_eq!(info.current_mode(), Some("bypassPermissions"));
    eprintln!(
        "session {} (resumed={}) agent={:?}",
        info.acp_session_id.0, info.resumed, info.agent
    );
    Session {
        handle,
        events: rx,
        timeout: REAL_TIMEOUT,
    }
}

/// Shuts down and checks that no process of the agent's group is left (npx → node → claude).
async fn stop(s: Session) {
    let pgid = s.handle.pid().expect("pid");
    s.handle.shutdown().await;
    assert_group_gone(pgid, Duration::from_secs(10)).await;
}

fn summarize(events: &[AgentEvent]) -> Vec<&'static str> {
    events
        .iter()
        .map(|e| match e {
            AgentEvent::Ready(_) => "ready",
            AgentEvent::Output(o) => match o {
                AgentOutput::MessageChunk(_) => "message",
                AgentOutput::ThoughtChunk(_) => "thought",
                AgentOutput::ToolCall(_) => "tool_call",
                AgentOutput::ToolCallUpdate(_) => "tool_call_update",
                AgentOutput::Usage(_) => "usage",
                AgentOutput::Unknown(_) => "unknown",
                AgentOutput::AvailableCommands(_) => "commands",
                AgentOutput::ModeChanged(_) => "mode",
                AgentOutput::ConfigOptions(_) => "config",
                AgentOutput::SessionInfo(_) => "session_info",
                AgentOutput::Plan(_) => "plan",
                AgentOutput::UserMessageChunk(_) => "user_message",
            },
            AgentEvent::PermissionAutoAnswered { .. } => "permission",
            _ => "other",
        })
        .collect()
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_prompt_streams_on_haiku() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut s = start_claude(dir.path(), SpawnOptions::default()).await;
    s.handle
        .prompt_text("Reply with exactly one word: pong")
        .expect("prompt");
    let (events, stop_reason) = s.turn().await;
    let text = message_text(&events);
    eprintln!("events: {:?}\ntext: {text:?}", summarize(&events));
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::Output(AgentOutput::MessageChunk(_))))
    );
    assert!(text.to_lowercase().contains("pong"), "{text}");
    stop(s).await;
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_cancel_mid_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut s = start_claude(dir.path(), SpawnOptions::default()).await;
    s.handle
        .prompt_text("Without using any tools, write a 1500-word story about a lighthouse keeper.")
        .expect("prompt");
    let first = s.until_message("").await;
    eprintln!("first chunk: {first:?}");
    let t0 = Instant::now();
    s.handle.cancel().expect("cancel");
    let (events, stop_reason) = s.turn().await;
    eprintln!(
        "cancelled after {:?}; {} more events",
        t0.elapsed(),
        events.len()
    );
    assert_eq!(stop_reason, Ok(StopReason::Cancelled));
    assert!(t0.elapsed() < Duration::from_secs(30));
    stop(s).await;
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_bypass_mode_writes_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut s = start_claude(dir.path(), SpawnOptions::default()).await;
    s.handle
        .prompt_text(
            "Create a file named hello.txt in the current directory containing exactly: hi",
        )
        .expect("prompt");
    let (events, stop_reason) = s.turn().await;
    eprintln!("events: {:?}", summarize(&events));
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    let body =
        std::fs::read_to_string(dir.path().join("hello.txt")).expect("hello.txt was created");
    assert_eq!(body.trim(), "hi");
    for e in &events {
        if let AgentEvent::PermissionAutoAnswered { chosen, .. } = e {
            eprintln!("permission asked; chosen = {chosen:?}");
            assert!(matches!(
                chosen,
                Some(PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways)
            ));
        }
    }
    stop(s).await;
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_session_load_restores_conversation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut s = start_claude(dir.path(), SpawnOptions::default()).await;
    let session = s.handle.info().acp_session_id.clone();
    s.handle
        .prompt_text("Remember the codeword PINEAPPLE-42. Reply only: ok")
        .expect("prompt");
    assert_eq!(s.turn().await.1, Ok(StopReason::EndTurn));
    stop(s).await;

    let opts = SpawnOptions {
        resume: Some(session.clone()),
        ..SpawnOptions::default()
    };
    let mut s = start_claude(dir.path(), opts).await;
    assert!(s.handle.info().resumed);
    assert_eq!(s.handle.info().acp_session_id, session);
    s.handle
        .prompt_text("What was the codeword? Reply with only the codeword.")
        .expect("prompt");
    let (events, stop_reason) = s.turn().await;
    let text = message_text(&events);
    eprintln!("answer: {text:?}");
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    assert!(text.contains("PINEAPPLE-42"), "{text}");
    stop(s).await;
}
