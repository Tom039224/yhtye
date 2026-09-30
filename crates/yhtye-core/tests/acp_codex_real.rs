//! Real Codex tests (`@agentclientprotocol/codex-acp` 2.0.0 through `npx`, always
//! `nvidia/nemotron-3-super-120b-a12b:free` through OpenRouter with the user's `~/.codex`
//! configuration). Ignored by default; the OpenRouter key must be in the
//! environment (`OPENROUTER_API_KEY_CODEX`, exported by the user's fish config), so run
//! `fish -c 'cargo test -p yhtye-core --test acp_codex_real -- --ignored --test-threads=1 --nocapture'`.
//! Prompts are kept tiny. Findings: `docs/architecture/acp-harnesses.md` §9.

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use common::codex::{
    MODEL, REAL_TIMEOUT, assert_no_new_processes, codex_harness, codex_home, codex_preset,
    codex_processes,
};
use common::{Session, assert_group_gone, message_text};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use yhtye_core::acp::schema::StopReason;
use yhtye_core::acp::{AgentEvent, AgentOutput, HarnessConfig, SpawnOptions, spawn_agent};
use yhtye_core::domain::{Role, StepKind, TaskKind, ToolError};
use yhtye_core::mcp::{McpHost, SessionBinding, TokenRegistry, ToolCall, ToolPort};
use yhtye_core::prompts::{StepPrompt, step_prompt, system_prompt};

/// A running Codex session plus what is needed to check its cleanup.
struct Cx {
    s: Session,
    /// Events that arrived before `Ready` (e.g. history replayed by `session/load`).
    before_ready: Vec<AgentEvent>,
    processes_before: BTreeSet<u32>,
}

/// Starts Codex on [`MODEL`] in `agent-full-access` mode and asserts both are in effect.
async fn start(harness: &HarnessConfig, cwd: &Path, options: SpawnOptions) -> Cx {
    let processes_before = codex_processes();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let handle = spawn_agent(harness, cwd, options, tx)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let info = handle.info();
    assert_eq!(
        info.config_value("model"),
        Some(MODEL),
        "model must be {MODEL}: {:?}",
        info.config_options
    );
    assert_eq!(info.current_mode(), Some("agent-full-access"));
    eprintln!(
        "session {} (resumed={}) agent={:?} load_session={} mode={:?}",
        info.acp_session_id.0,
        info.resumed,
        info.agent,
        info.capabilities.load_session,
        info.current_mode()
    );
    let mut before_ready = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if matches!(ev, AgentEvent::Ready(_)) {
            break;
        }
        if !matches!(ev, AgentEvent::Stderr(_)) {
            before_ready.push(ev);
        }
    }
    Cx {
        s: Session {
            handle,
            events: rx,
            timeout: REAL_TIMEOUT,
        },
        before_ready,
        processes_before,
    }
}

async fn start_default(cwd: &Path, options: SpawnOptions) -> Cx {
    start(&codex_harness(), cwd, options).await
}

/// Shuts down; no process of the agent's group (the adapter, `codex.js`, the
/// `codex app-server` it started) may be left, and the user's daemon is untouched.
async fn stop(cx: Cx) {
    let pgid = cx.s.handle.pid().expect("pid");
    cx.s.handle.shutdown().await;
    assert_group_gone(pgid, Duration::from_secs(10)).await;
    assert_no_new_processes(&cx.processes_before, Duration::from_secs(10)).await;
}

fn kind(e: &AgentEvent) -> &'static str {
    match e {
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
    }
}

fn summarize(events: &[AgentEvent]) -> Vec<&'static str> {
    events.iter().map(kind).collect()
}

/// The text of a turn without Codex's per-turn "Model metadata ... not found"
/// warning (it is injected as a message for models outside the OpenAI catalog).
fn answer(events: &[AgentEvent]) -> String {
    message_text(events)
}

#[tokio::test]
#[ignore = "real Codex (nemotron-3-super free via OpenRouter)"]
async fn real_codex_prompt_streams() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cx = start_default(dir.path(), SpawnOptions::default()).await;
    cx.s.handle
        .prompt_text("Reply with exactly one word: pong")
        .expect("prompt");
    let (events, stop_reason) = cx.s.turn().await;
    let text = answer(&events);
    eprintln!("events: {:?}\ntext: {text:?}", summarize(&events));
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    assert!(text.to_lowercase().contains("pong"), "{text}");
    stop(cx).await;
}

/// The agent process starts in `$HOME`; the session directory reaches Codex only
/// through ACP `cwd`. Files and shell commands must land there, without any
/// approval (`agent-full-access`).
#[tokio::test]
#[ignore = "real Codex (nemotron-3-super free via OpenRouter)"]
async fn real_codex_works_in_the_acp_cwd_without_approvals() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cx = start_default(dir.path(), SpawnOptions::default()).await;
    cx.s.handle
        .prompt_text(
            "Run the shell command `pwd > where.txt` and then the shell command \
             `echo hi > hello.txt`. Use your shell tool for both.",
        )
        .expect("prompt");
    let (events, stop_reason) = cx.s.turn().await;
    eprintln!("events: {:?}", summarize(&events));
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    let body = std::fs::read_to_string(dir.path().join("hello.txt")).expect("hello.txt");
    assert_eq!(body.trim(), "hi");
    let pwd = std::fs::read_to_string(dir.path().join("where.txt")).expect("where.txt");
    let real = std::fs::canonicalize(dir.path()).expect("canonicalize");
    assert_eq!(Path::new(pwd.trim()), real, "shell ran in {pwd:?}");
    assert!(
        !summarize(&events).contains(&"permission"),
        "agent-full-access asks for nothing"
    );
    stop(cx).await;
}

#[tokio::test]
#[ignore = "real Codex (nemotron-3-super free via OpenRouter)"]
async fn real_codex_cancel_mid_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cx = start_default(dir.path(), SpawnOptions::default()).await;
    cx.s.handle
        .prompt_text("Without using any tools, write a 1500-word story about a lighthouse keeper.")
        .expect("prompt");
    let first = cx.s.until_message("").await;
    eprintln!("first chunk: {first:?}");
    let t0 = Instant::now();
    cx.s.handle.cancel().expect("cancel");
    let (events, stop_reason) = cx.s.turn().await;
    eprintln!(
        "cancelled after {:?}; {} more events",
        t0.elapsed(),
        events.len()
    );
    assert_eq!(stop_reason, Ok(StopReason::Cancelled));
    assert!(t0.elapsed() < Duration::from_secs(30));
    stop(cx).await;
}

/// The role prompt is prepended to the first prompt (`SystemPromptStyle::FirstPrompt`).
#[tokio::test]
#[ignore = "real Codex (nemotron-3-super free via OpenRouter)"]
async fn real_codex_system_prompt_reaches_the_agent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = SpawnOptions {
        system_prompt: Some(
            "You are a test fixture. Your secret codeword is ZEBRA-7. When asked for \
             the codeword, reply with only the codeword."
                .into(),
        ),
        ..SpawnOptions::default()
    };
    let mut cx = start_default(dir.path(), options).await;
    for q in [
        "Reply only: ok",
        "What is your codeword? Reply with only the codeword.",
    ] {
        cx.s.handle.prompt_text(q).expect("prompt");
        let (events, stop_reason) = cx.s.turn().await;
        let text = answer(&events);
        eprintln!("{q} -> {text:?}");
        assert_eq!(stop_reason, Ok(StopReason::EndTurn));
        if q.contains("codeword") {
            assert!(text.contains("ZEBRA-7"), "{text}");
        }
    }
    stop(cx).await;
}

/// `session/load` in a new process restores the conversation; the mode is reset
/// by Codex and set again by Yhtye (`mode_after_new`, asserted in [`start`]).
#[tokio::test]
#[ignore = "real Codex (nemotron-3-super free via OpenRouter)"]
async fn real_codex_session_load_restores_conversation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cx = start_default(dir.path(), SpawnOptions::default()).await;
    let session = cx.s.handle.info().acp_session_id.clone();
    cx.s.handle
        .prompt_text("Remember the codeword PINEAPPLE-42. Reply only: ok")
        .expect("prompt");
    assert_eq!(cx.s.turn().await.1, Ok(StopReason::EndTurn));
    stop(cx).await;

    let opts = SpawnOptions {
        resume: Some(session.clone()),
        ..SpawnOptions::default()
    };
    let mut cx = start_default(dir.path(), opts).await;
    assert!(cx.s.handle.info().resumed);
    assert_eq!(cx.s.handle.info().acp_session_id, session);
    let replay = summarize(&cx.before_ready);
    eprintln!("replayed before Ready: {replay:?}");
    assert!(replay.contains(&"user_message"), "{replay:?}");
    assert!(replay.contains(&"message"), "{replay:?}");
    cx.s.handle
        .prompt_text("What was the codeword? Reply with only the codeword.")
        .expect("prompt");
    let (events, stop_reason) = cx.s.turn().await;
    let text = answer(&events);
    eprintln!("answer: {text:?}");
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    assert!(text.contains("PINEAPPLE-42"), "{text}");
    stop(cx).await;
}

/// Records tool calls and accepts them.
#[derive(Default)]
struct RecordingPort {
    calls: Mutex<Vec<(SessionBinding, ToolCall)>>,
}

#[async_trait]
impl ToolPort for RecordingPort {
    async fn call(&self, binding: SessionBinding, call: ToolCall) -> Result<Value, ToolError> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((binding, call));
        Ok(json!({ "ok": true }))
    }
}

/// Stage 2 for Codex: an implementer with Yhtye's HTTP MCP server (passed in
/// `session/new`) does the step and calls `report_step_done` through MCP.
#[tokio::test]
#[ignore = "real Codex (nemotron-3-super free via OpenRouter)"]
async fn real_codex_implementer_reports_through_mcp() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("README.md"), "# demo\n").expect("write");
    let port = Arc::new(RecordingPort::default());
    let host = McpHost::start(
        "127.0.0.1:0".parse().expect("addr"),
        TokenRegistry::new(),
        port.clone(),
        None,
    )
    .await
    .expect("mcp host");
    let token = host.registry().issue(SessionBinding {
        session: "T-1/0".into(),
        role: Role::Implementer,
        project: "P-1".into(),
        chat: None,
        group: Some("G-1".into()),
        task: Some("T-1".into()),
        step: Some(0),
    });
    let options = SpawnOptions {
        mcp_servers: vec![host.acp_server(&token)],
        system_prompt: Some(system_prompt(Role::Implementer).to_string()),
        ..SpawnOptions::default()
    };
    let mut cx = start_default(dir.path(), options).await;
    let prompt = step_prompt(&StepPrompt {
        task_id: "T-1",
        task_title: "Append a line to README.md",
        task_kind: TaskKind::Code,
        step_index: 0,
        step_count: 1,
        step_kind: StepKind::Implement,
        instruction: "Append the line `hello from yhtye` to README.md.",
        context: None,
        note: None,
        review_range: None,
    });
    cx.s.handle.prompt_text(prompt).expect("prompt");
    let (events, stop_reason) = cx.s.turn().await;
    eprintln!(
        "events: {:?}\ntext: {:?}",
        summarize(&events),
        answer(&events)
    );
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    let calls = port
        .calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    eprintln!("MCP calls: {calls:#?}");
    assert!(
        calls
            .iter()
            .any(|(b, c)| b.session == "T-1/0" && matches!(c, ToolCall::ReportStepDone(_))),
        "{calls:?}"
    );
    let body = std::fs::read_to_string(dir.path().join("README.md")).expect("README");
    assert!(body.contains("hello from yhtye"), "{body}");
    stop(cx).await;
    host.shutdown().await;
}

/// With the OpenRouter provider in the user's Codex config the model list is
/// OpenRouter's public list: no prompt, no model call, no credentials in the
/// request. Contains the test model with its efforts.
#[tokio::test]
#[ignore = "real OpenRouter model list (public, no credentials)"]
async fn real_codex_model_list_comes_from_openrouter() {
    use yhtye_core::agents::ModelService;
    let dir = tempfile::tempdir().expect("tempdir");
    let home = codex_home().display().to_string();
    let svc = ModelService::new(
        dir.path().join("probe"),
        Arc::new(yhtye_core::secrets::Secrets::none()),
    )
    .with_env(move |k| (k == "CODEX_HOME").then(|| home.clone().into()));
    let preset = codex_preset();
    let started = Instant::now();
    let listed = svc.get(&preset, false).await.expect("models");
    eprintln!(
        "{} models in {:?}, e.g. {:?}",
        listed.models.len(),
        started.elapsed(),
        listed
            .models
            .iter()
            .take(3)
            .map(|m| &m.value)
            .collect::<Vec<_>>()
    );
    assert!(listed.models.len() > 50, "a long list (searched in the UI)");
    let test_model = listed
        .models
        .iter()
        .find(|m| m.value == MODEL)
        .expect("the test model is listed");
    let efforts: Vec<&str> = test_model
        .efforts
        .as_ref()
        .expect("read")
        .iter()
        .map(|e| e.value.as_str())
        .collect();
    assert_eq!(
        efforts,
        ["medium", "low"],
        "OpenRouter's efforts of the test model"
    );
    let with_efforts = listed
        .models
        .iter()
        .find(|m| m.efforts.as_ref().is_some_and(|e| !e.is_empty()))
        .expect("some model lists efforts");
    let efforts = svc
        .get_efforts(&preset, &with_efforts.value)
        .await
        .expect("efforts");
    eprintln!(
        "efforts of {}: {:?}",
        with_efforts.value,
        efforts.iter().map(|e| e.value.as_str()).collect::<Vec<_>>()
    );
    assert!(!efforts.is_empty());
}

/// The adapter's own listing (what other providers get): a short session
/// without a prompt lists OpenAI's Codex catalog, and stops cleanly.
#[tokio::test]
#[ignore = "real Codex adapter (no model call)"]
async fn real_codex_probe_lists_the_adapters_models() {
    let before = codex_processes();
    let dir = tempfile::tempdir().expect("tempdir");
    let preset = codex_preset();
    let listed = yhtye_core::agents::probe_models(
        &yhtye_core::secrets::Secrets::none(),
        &preset,
        dir.path(),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("models");
    eprintln!(
        "{} models, current {:?}: {:?}",
        listed.models.len(),
        listed.current,
        listed
            .models
            .iter()
            .map(|m| (&m.value, m.efforts.as_ref().map(Vec::len)))
            .collect::<Vec<_>>()
    );
    assert!(!listed.models.is_empty());
    assert_no_new_processes(&before, Duration::from_secs(10)).await;
}

/// The effort chosen in Yhtye reaches Codex through `CODEX_CONFIG`
/// (`model_reasoning_effort`): the session's rollout in `~/.codex/sessions`
/// records the effort of the turn. One tiny prompt.
#[tokio::test]
#[ignore = "real Codex (nemotron-3-super free via OpenRouter)"]
async fn real_codex_effort_from_codex_config_is_applied() {
    let dir = tempfile::tempdir().expect("tempdir");
    let harness = codex_preset().config(
        yhtye_core::agents::AgentRole::Implementer,
        Some(MODEL),
        Some("low"),
    );
    let mut cx = start(&harness, dir.path(), SpawnOptions::default()).await;
    let session = cx.s.handle.info().acp_session_id.0.to_string();
    cx.s.handle
        .prompt_text("Reply with exactly one word: pong")
        .expect("prompt");
    let (events, stop_reason) = cx.s.turn().await;
    eprintln!("answer: {:?}", answer(&events));
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    stop(cx).await;
    let rollout = find_rollout(&codex_home().join("sessions"), &session).expect("rollout file");
    let text = std::fs::read_to_string(&rollout).expect("read rollout");
    let efforts: Vec<String> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["type"] == "turn_context")
        .filter_map(|v| {
            let p = &v["payload"];
            p["effort"]
                .as_str()
                .or_else(|| p["collaboration_mode"]["settings"]["reasoning_effort"].as_str())
                .map(str::to_string)
        })
        .collect();
    eprintln!("efforts recorded by Codex: {efforts:?}");
    assert!(
        !efforts.is_empty() && efforts.iter().all(|e| e == "low"),
        "{efforts:?}"
    );
}

/// The rollout file of `session` under Codex's `sessions` directory.
fn find_rollout(dir: &Path, session: &str) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        if path.is_dir() {
            if let Some(found) = find_rollout(&path, session) {
                return Some(found);
            }
        } else if path.file_name()?.to_string_lossy().contains(session) {
            return Some(path);
        }
    }
    None
}
