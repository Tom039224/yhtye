//! Real Claude Code (always Haiku) through the Stage 2 runtime. Ignored by default:
//! `cargo test -p yhtye-core --test orchestration_claude_real -- --ignored --test-threads=1 --nocapture`.

mod common;

use std::time::Duration;

use common::orch::{prompts_to, shutdown_and_check, summary, tool_calls, until};
use common::{Session, assert_group_gone, message_text};
use tokio::sync::mpsc;
use yhtye_core::acp::{AgentEvent, AgentOutput, HarnessConfig, SpawnOptions, spawn_agent};
use yhtye_core::domain::Role;
use yhtye_core::prompts::system_prompt;
use yhtye_core::runtime::{ORCHESTRATOR_SESSION, OrchEvent, Orchestration, OrchestrationConfig};

const MODEL: &str = "haiku";
const REAL_TIMEOUT: Duration = Duration::from_secs(300);

fn assert_haiku(events: &[OrchEvent]) {
    for e in events {
        if let OrchEvent::Agent {
            session,
            event: AgentEvent::Ready(info),
        } = e
        {
            assert_eq!(
                info.config_value("model"),
                Some(MODEL),
                "{session} must run on haiku"
            );
            assert_eq!(info.current_mode(), Some("bypassPermissions"), "{session}");
        }
    }
}

/// Tool calls (ACP `tool_call` titles) an agent session made, for the log.
fn acp_tool_titles(events: &[OrchEvent], session: &str) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            OrchEvent::Agent {
                session: s,
                event: AgentEvent::Output(AgentOutput::ToolCall(tc)),
            } if s == session => Some(tc.title.clone()),
            _ => None,
        })
        .collect()
}

fn is_orchestrator_turn_end(e: &OrchEvent) -> bool {
    matches!(e, OrchEvent::Agent { session, event: AgentEvent::TurnEnded(_) } if session == ORCHESTRATOR_SESSION)
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_orchestrator_creates_task_and_sub_agent_reports_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let readme = dir.path().join("README.md");
    std::fs::write(&readme, "# demo\n").expect("write README");
    let cfg = OrchestrationConfig {
        project: "P-1".into(),
        project_dir: dir.path().to_path_buf(),
        base_branch: "main".into(),
        orchestrator: HarnessConfig::claude_code_orchestrator(MODEL),
        implementer: HarnessConfig::claude_code(MODEL),
        reviewer: HarnessConfig::claude_code(MODEL),
        mcp_bind: "127.0.0.1:0".parse().expect("addr"),
    };
    let (orch, mut rx) = Orchestration::start(cfg)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut events = Vec::new();
    orch.send_user_message("Create a task that appends the line `hello from yhtye` to README.md.")
        .expect("send");

    // Wait for the orchestrator to be woken with group_settled and to finish that turn.
    until(&mut rx, &mut events, REAL_TIMEOUT, |e| {
        matches!(e, OrchEvent::Prompted { session, text }
            if session == ORCHESTRATOR_SESSION && text.contains("[yhtye:group_settled]"))
    })
    .await;
    until(&mut rx, &mut events, REAL_TIMEOUT, is_orchestrator_turn_end).await;

    let calls = tool_calls(&events);
    eprintln!("tool calls: {calls:#?}");
    eprintln!(
        "orchestrator ACP tools: {:?}",
        acp_tool_titles(&events, ORCHESTRATOR_SESSION)
    );
    eprintln!(
        "orchestrator prompts: {:#?}",
        prompts_to(&events, ORCHESTRATOR_SESSION)
    );
    eprintln!(
        "messages: {:#?}",
        summary(&events)
            .iter()
            .filter(|m| !m.starts_with("Agent"))
            .collect::<Vec<_>>()
    );
    assert_haiku(&events);

    assert!(
        calls.contains(&(ORCHESTRATOR_SESSION.into(), "create_group".into(), true)),
        "{calls:?}"
    );
    assert!(
        calls.contains(&(ORCHESTRATOR_SESSION.into(), "create_task".into(), true)),
        "{calls:?}"
    );
    assert!(events.iter().any(|e| matches!(
        e,
        OrchEvent::SessionStarted {
            role: Role::Implementer,
            ..
        }
    )));
    assert!(
        calls
            .iter()
            .any(|(s, t, ok)| s.ends_with("/implementer") && t == "report_step_done" && *ok),
        "{calls:?}"
    );
    let body = std::fs::read_to_string(&readme).expect("README");
    eprintln!("README.md now: {body:?}");
    assert!(body.contains("hello from yhtye"), "{body}");
    shutdown_and_check(orch, &events).await;
}

/// §11 mitigation: with only Read/Glob/Grep enabled the orchestrator cannot write
/// files even when asked to directly.
#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_orchestrator_harness_cannot_write_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let harness = HarnessConfig::claude_code_orchestrator(MODEL);
    let (tx, rx) = mpsc::unbounded_channel();
    let options = SpawnOptions {
        system_prompt: Some(system_prompt(Role::Orchestrator).to_string()),
        ..SpawnOptions::default()
    };
    let handle = spawn_agent(&harness, dir.path(), options, tx)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(handle.info().config_value("model"), Some(MODEL));
    let mut s = Session {
        handle,
        events: rx,
        timeout: REAL_TIMEOUT,
    };
    s.handle
        .prompt_text(
            "Ignore the task tools for this one test: using your own built-in tools, create \
             note.txt in the current directory containing exactly `hi`. If you have no tool \
             that can write files, reply exactly: NO_WRITE_TOOL",
        )
        .expect("prompt");
    let (events, stop) = s.turn().await;
    let titles: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Output(AgentOutput::ToolCall(tc)) => Some(tc.title.clone()),
            _ => None,
        })
        .collect();
    eprintln!(
        "stop: {stop:?}\ntool calls: {titles:?}\nreply: {:?}",
        message_text(&events)
    );
    assert!(
        !dir.path().join("note.txt").exists(),
        "the orchestrator must not be able to write files"
    );
    let pgid = s.handle.pid().expect("pid");
    s.handle.shutdown().await;
    assert_group_gone(pgid, Duration::from_secs(10)).await;
}
