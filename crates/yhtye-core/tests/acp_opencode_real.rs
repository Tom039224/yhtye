//! Real OpenCode tests (`opencode acp`, always `opencode/muse-spark-1.3-contributor-free`).
//! Ignored by default; run with
//! `cargo test -p yhtye-core --test acp_opencode_real -- --ignored --test-threads=1 --nocapture`.
//! Uses the local OpenCode installation and credentials. Prompts are kept tiny.
//! Findings: `docs/architecture/acp-harnesses.md` §7.

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use common::opencode::{MODEL, REAL_TIMEOUT, assert_no_new_servers, opencode_servers};
use common::{Session, assert_group_gone, message_text};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use yhtye_core::acp::schema::{PermissionOptionKind, StopReason};
use yhtye_core::acp::{AgentEvent, AgentOutput, HarnessConfig, SpawnOptions, spawn_agent};
use yhtye_core::domain::{Role, StepKind, TaskKind, ToolError};
use yhtye_core::mcp::{McpHost, SessionBinding, TokenRegistry, ToolCall, ToolPort};
use yhtye_core::prompts::{StepPrompt, step_prompt, system_prompt};

/// A running OpenCode session plus what is needed to check its cleanup.
struct Oc {
    s: Session,
    /// Events that arrived before `Ready` (e.g. history replayed by `session/load`).
    before_ready: Vec<AgentEvent>,
    servers_before: BTreeSet<u32>,
}

/// Starts OpenCode on [`MODEL`] in `build` mode and asserts both are in effect.
async fn start(harness: &HarnessConfig, cwd: &Path, options: SpawnOptions) -> Oc {
    let servers_before = opencode_servers();
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
    assert_eq!(info.current_mode(), Some("build"));
    eprintln!(
        "session {} (resumed={}) agent={:?} load_session={} modes={:?}",
        info.acp_session_id.0, info.resumed, info.agent, info.capabilities.load_session, info.modes
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
    Oc {
        s: Session {
            handle,
            events: rx,
            timeout: REAL_TIMEOUT,
        },
        before_ready,
        servers_before,
    }
}

async fn start_build(cwd: &Path, options: SpawnOptions) -> Oc {
    start(&HarnessConfig::opencode(MODEL), cwd, options).await
}

/// Shuts down; no process of the agent's group and no `opencode serve --stdio`
/// it started (a separate session) may be left.
async fn stop(oc: Oc) {
    let pgid = oc.s.handle.pid().expect("pid");
    let spawned: Vec<u32> = opencode_servers()
        .difference(&oc.servers_before)
        .copied()
        .collect();
    eprintln!("opencode servers started by this session: {spawned:?}");
    oc.s.handle.shutdown().await;
    assert_group_gone(pgid, Duration::from_secs(10)).await;
    assert_no_new_servers(&oc.servers_before, Duration::from_secs(10)).await;
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

/// `title (kind)` of each ACP tool call.
fn tool_titles(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Output(AgentOutput::ToolCall(tc)) => {
                Some(format!("{} ({:?})", tc.title, tc.kind))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
#[ignore = "real OpenCode (muse-spark free)"]
async fn real_opencode_prompt_streams() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut oc = start_build(dir.path(), SpawnOptions::default()).await;
    oc.s.handle
        .prompt_text("Reply with exactly one word: pong")
        .expect("prompt");
    let (events, stop_reason) = oc.s.turn().await;
    let text = message_text(&events);
    eprintln!("events: {:?}\ntext: {text:?}", summarize(&events));
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    assert!(text.to_lowercase().contains("pong"), "{text}");
    stop(oc).await;
}

/// The agent process starts in `$HOME`; the session directory reaches OpenCode
/// only through ACP `cwd`. Files and shell commands must land there.
#[tokio::test]
#[ignore = "real OpenCode (muse-spark free)"]
async fn real_opencode_works_in_the_acp_cwd() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut oc = start_build(dir.path(), SpawnOptions::default()).await;
    oc.s.handle
        .prompt_text(
            "Create a file named hello.txt in the current directory containing exactly: hi\n\
             Then run the shell command `pwd > where.txt`.",
        )
        .expect("prompt");
    let (events, stop_reason) = oc.s.turn().await;
    eprintln!(
        "events: {:?}\ntools: {:?}",
        summarize(&events),
        tool_titles(&events)
    );
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    let body =
        std::fs::read_to_string(dir.path().join("hello.txt")).expect("hello.txt was created");
    assert_eq!(body.trim(), "hi");
    let pwd = std::fs::read_to_string(dir.path().join("where.txt")).expect("where.txt");
    let real = std::fs::canonicalize(dir.path()).expect("canonicalize");
    assert_eq!(Path::new(pwd.trim()), real, "shell ran in {pwd:?}");
    for e in &events {
        if let AgentEvent::PermissionAutoAnswered {
            chosen, options, ..
        } = e
        {
            eprintln!("permission asked: {options:?}; chosen = {chosen:?}");
            assert!(matches!(
                chosen,
                Some(PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways)
            ));
        }
    }
    // OpenCode must not leave its own files in the project directory.
    let mut left: Vec<String> = std::fs::read_dir(dir.path())
        .expect("dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["hello.txt", "where.txt"]);
    stop(oc).await;
}

/// Accessing a path outside the session directory asks for permission
/// (`external_directory`); Yhtye answers `always` automatically.
#[tokio::test]
#[ignore = "real OpenCode (muse-spark free)"]
async fn real_opencode_external_directory_permission_is_auto_approved() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("tempdir");
    let target = outside.path().join("note.txt");
    std::fs::write(&target, "secret-word: KIWI-9\n").expect("write");
    let mut oc = start_build(dir.path(), SpawnOptions::default()).await;
    oc.s.handle
        .prompt_text(format!(
            "Read the file {} with your read tool and reply with only the secret word.",
            target.display()
        ))
        .expect("prompt");
    let (events, stop_reason) = oc.s.turn().await;
    let text = message_text(&events);
    eprintln!("events: {:?}\ntext: {text:?}", summarize(&events));
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    let asked: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::PermissionAutoAnswered {
                tool_call,
                options,
                chosen,
            } => Some((tool_call.fields.title.clone(), options.clone(), *chosen)),
            _ => None,
        })
        .collect();
    eprintln!("permissions: {asked:#?}");
    assert!(
        !asked.is_empty(),
        "expected an external_directory permission"
    );
    assert!(
        asked
            .iter()
            .all(|(_, _, c)| *c == Some(PermissionOptionKind::AllowAlways))
    );
    assert!(text.contains("KIWI-9"), "{text}");
    stop(oc).await;
}

#[tokio::test]
#[ignore = "real OpenCode (muse-spark free)"]
async fn real_opencode_cancel_mid_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut oc = start_build(dir.path(), SpawnOptions::default()).await;
    oc.s.handle
        .prompt_text("Without using any tools, write a 1500-word story about a lighthouse keeper.")
        .expect("prompt");
    let first = oc.s.until_message("").await;
    eprintln!("first chunk: {first:?}");
    let t0 = Instant::now();
    oc.s.handle.cancel().expect("cancel");
    let (events, stop_reason) = oc.s.turn().await;
    eprintln!(
        "cancelled after {:?}; {} more events",
        t0.elapsed(),
        events.len()
    );
    assert_eq!(stop_reason, Ok(StopReason::Cancelled));
    assert!(t0.elapsed() < Duration::from_secs(30));
    stop(oc).await;
}

/// The role prompt is prepended to the first prompt (`SystemPromptStyle::FirstPrompt`);
/// it is part of the conversation, so later turns still know it.
#[tokio::test]
#[ignore = "real OpenCode (muse-spark free)"]
async fn real_opencode_system_prompt_reaches_the_agent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = SpawnOptions {
        system_prompt: Some(
            "You are a test fixture. Your secret codeword is ZEBRA-7. When asked for \
             the codeword, reply with only the codeword."
                .into(),
        ),
        ..SpawnOptions::default()
    };
    let mut oc = start_build(dir.path(), options).await;
    for q in [
        "Reply only: ok",
        "What is your codeword? Reply with only the codeword.",
    ] {
        oc.s.handle.prompt_text(q).expect("prompt");
        let (events, stop_reason) = oc.s.turn().await;
        let text = message_text(&events);
        eprintln!("{q} -> {text:?}");
        assert_eq!(stop_reason, Ok(StopReason::EndTurn));
        if q.contains("codeword") {
            assert!(text.contains("ZEBRA-7"), "{text}");
        }
    }
    stop(oc).await;
}

#[tokio::test]
#[ignore = "real OpenCode (muse-spark free)"]
async fn real_opencode_session_load_restores_conversation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut oc = start_build(dir.path(), SpawnOptions::default()).await;
    let session = oc.s.handle.info().acp_session_id.clone();
    oc.s.handle
        .prompt_text("Remember the codeword PINEAPPLE-42. Reply only: ok")
        .expect("prompt");
    assert_eq!(oc.s.turn().await.1, Ok(StopReason::EndTurn));
    stop(oc).await;

    let opts = SpawnOptions {
        resume: Some(session.clone()),
        ..SpawnOptions::default()
    };
    let mut oc = start_build(dir.path(), opts).await;
    assert!(oc.s.handle.info().resumed);
    assert_eq!(oc.s.handle.info().acp_session_id, session);
    let replay = summarize(&oc.before_ready);
    eprintln!("replayed before Ready: {replay:?}");
    assert!(replay.contains(&"user_message"), "{replay:?}");
    assert!(replay.contains(&"message"), "{replay:?}");
    let first_user = replay.iter().position(|k| *k == "user_message");
    let first_agent = replay.iter().position(|k| *k == "message");
    assert!(first_user < first_agent, "history is replayed in order");
    oc.s.handle
        .prompt_text("What was the codeword? Reply with only the codeword.")
        .expect("prompt");
    let (events, stop_reason) = oc.s.turn().await;
    let text = message_text(&events);
    eprintln!("answer: {text:?}");
    assert_eq!(stop_reason, Ok(StopReason::EndTurn));
    assert!(text.contains("PINEAPPLE-42"), "{text}");
    stop(oc).await;
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

/// Stage 2 for OpenCode: an implementer with Yhtye's HTTP MCP server (passed in
/// `session/new`) does the step and calls `report_step_done` through MCP.
#[tokio::test]
#[ignore = "real OpenCode (muse-spark free)"]
async fn real_opencode_implementer_reports_through_mcp() {
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
        group: Some("G-1".into()),
        task: Some("T-1".into()),
        step: Some(0),
    });
    let options = SpawnOptions {
        mcp_servers: vec![host.acp_server(&token)],
        system_prompt: Some(system_prompt(Role::Implementer).to_string()),
        resume: None,
    };
    let mut oc = start_build(dir.path(), options).await;
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
    oc.s.handle.prompt_text(prompt).expect("prompt");
    let (events, stop_reason) = oc.s.turn().await;
    eprintln!(
        "events: {:?}\ntools: {:?}\ntext: {:?}",
        summarize(&events),
        tool_titles(&events),
        message_text(&events)
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
    for e in &events {
        if let AgentEvent::Output(AgentOutput::ToolCallUpdate(u)) = e
            && u.fields.raw_input.is_some()
        {
            eprintln!("tool update: {:?} {:?}", u.fields.title, u.fields.raw_input);
        }
    }
    let body = std::fs::read_to_string(dir.path().join("README.md")).expect("README");
    assert!(body.contains("hello from yhtye"), "{body}");
    // OpenCode adds the MCP server at runtime; nothing is written to the project.
    let left: Vec<String> = std::fs::read_dir(dir.path())
        .expect("dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["README.md"]);
    stop(oc).await;
    host.shutdown().await;
}
