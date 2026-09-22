//! Real Claude Code (always Haiku) through the runtime and state machine. Ignored by default:
//! `cargo test -p yhtye-core --test orchestration_claude_real -- --ignored --test-threads=1 --nocapture`.

mod common;

use std::time::Duration;

use common::orch::{config, prompts_to, shutdown_and_check, summary, tool_calls, until};
use common::{Session, assert_group_gone, message_text};
use tokio::sync::mpsc;
use yhtye_core::acp::{AgentEvent, AgentOutput, HarnessConfig, SpawnOptions, spawn_agent};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{DomainEvent, Role, TaskStatus};
use yhtye_core::prompts::system_prompt;
use yhtye_core::runtime::{ORCHESTRATOR_SESSION, Orchestration, OrchestrationConfig};

const MODEL: &str = "haiku";
const REAL_TIMEOUT: Duration = Duration::from_secs(300);

fn assert_haiku(events: &[ApiEvent]) {
    for e in events {
        if let ApiEventBody::Agent {
            session,
            event: AgentEvent::Ready(info),
        } = &e.body
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
fn acp_tool_titles(events: &[ApiEvent], session: &str) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::Agent {
                session: s,
                event: AgentEvent::Output(AgentOutput::ToolCall(tc)),
            } if s == session => Some(tc.title.clone()),
            _ => None,
        })
        .collect()
}

fn is_orchestrator_turn_end(e: &ApiEvent) -> bool {
    matches!(&e.body, ApiEventBody::Agent { session, event: AgentEvent::TurnEnded(_) } if session == ORCHESTRATOR_SESSION)
}

fn real_config(dir: &std::path::Path) -> OrchestrationConfig {
    config(
        dir,
        HarnessConfig::claude_code_orchestrator(MODEL),
        HarnessConfig::claude_code(MODEL),
        HarnessConfig::claude_code(MODEL),
    )
}

/// Sends `request` and waits until the orchestrator has handled `group_settled`.
async fn run_request(
    orch: &Orchestration,
    rx: &mut mpsc::UnboundedReceiver<ApiEvent>,
    request: &str,
) -> Vec<ApiEvent> {
    let mut events = Vec::new();
    orch.send_user_message(request).expect("send");
    until(rx, &mut events, REAL_TIMEOUT, |e| {
        matches!(&e.body, ApiEventBody::Prompted { session, text }
            if session == ORCHESTRATOR_SESSION && text.contains("[yhtye:group_settled]"))
    })
    .await;
    until(rx, &mut events, REAL_TIMEOUT, is_orchestrator_turn_end).await;
    events
}

fn log_run(events: &[ApiEvent]) {
    eprintln!("tool calls: {:#?}", tool_calls(events));
    eprintln!(
        "orchestrator ACP tools: {:?}",
        acp_tool_titles(events, ORCHESTRATOR_SESSION)
    );
    eprintln!(
        "orchestrator prompts: {:#?}",
        prompts_to(events, ORCHESTRATOR_SESSION)
    );
    eprintln!(
        "messages: {:#?}",
        summary(events)
            .iter()
            .filter(|m| !m.starts_with("Agent") && !m.starts_with("Domain"))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_orchestrator_creates_task_and_sub_agent_reports_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let readme = dir.path().join("README.md");
    std::fs::write(&readme, "# demo\n").expect("write README");
    let (orch, mut rx) = Orchestration::start(real_config(dir.path()))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let events = run_request(
        &orch,
        &mut rx,
        "Create a task that appends the line `hello from yhtye` to README.md.",
    )
    .await;
    log_run(&events);
    assert_haiku(&events);
    let calls = tool_calls(&events);

    assert!(
        calls.contains(&(ORCHESTRATOR_SESSION.into(), "create_group".into(), true)),
        "{calls:?}"
    );
    assert!(
        calls.contains(&(ORCHESTRATOR_SESSION.into(), "create_task".into(), true)),
        "{calls:?}"
    );
    assert!(events.iter().any(|e| matches!(
        &e.body,
        ApiEventBody::SessionStarted {
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

/// A `code` task with implement → review: the review runs in its own, separate
/// reviewer session and reports a verdict (Stage 3a).
#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_implement_then_review_uses_a_separate_reviewer_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let readme = dir.path().join("README.md");
    std::fs::write(&readme, "# demo\n").expect("write README");
    let (orch, mut rx) = Orchestration::start(real_config(dir.path()))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let events = run_request(
        &orch,
        &mut rx,
        "Create one code task with steps implement then review (steps: \
         [{\"kind\":\"implement\"},{\"kind\":\"review\"}]) that appends the line \
         `reviewed by yhtye` to README.md.",
    )
    .await;
    log_run(&events);
    assert_haiku(&events);
    let calls = tool_calls(&events);

    let reviewer_sessions: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted {
                session,
                role: Role::Reviewer,
                ..
            } => Some(session.as_str()),
            _ => None,
        })
        .collect();
    eprintln!("reviewer sessions: {reviewer_sessions:?}");
    assert!(
        !reviewer_sessions.is_empty(),
        "a reviewer session was started"
    );
    assert!(reviewer_sessions.iter().all(|s| s.contains("/review-")));
    let verdicts: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::ToolCalled { record }
                if record.tool == "report_step_done"
                    && record.binding.role == Role::Reviewer
                    && record.result.is_ok() =>
            {
                Some(record.args["verdict"].to_string())
            }
            _ => None,
        })
        .collect();
    eprintln!("reviewer verdicts: {verdicts:?}");
    assert!(
        !verdicts.is_empty(),
        "the reviewer reported a verdict: {calls:?}"
    );
    assert!(events.iter().any(|e| matches!(
        &e.body,
        ApiEventBody::Domain {
            event: DomainEvent::TaskStatusChanged {
                status: TaskStatus::Done,
                ..
            }
        }
    )));
    let body = std::fs::read_to_string(&readme).expect("README");
    eprintln!("README.md now: {body:?}");
    assert!(body.contains("reviewed by yhtye"), "{body}");
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
