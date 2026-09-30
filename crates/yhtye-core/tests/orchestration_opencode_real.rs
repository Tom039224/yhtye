//! Real OpenCode (always `opencode/muse-spark-1.3-contributor-free`) in every role,
//! through the runtime and state machine. Ignored by default:
//! `cargo test -p yhtye-core --test orchestration_opencode_real -- --ignored --test-threads=1 --nocapture`.

mod common;

use common::orch::ORCHESTRATOR_SESSION;

use common::opencode::{
    MODEL, REAL_TIMEOUT, assert_model, assert_no_new_servers, opencode_harness, opencode_servers,
    real_config,
};
use common::orch::{prompts_to, shutdown_and_check, summary, tool_calls, until_healthy};
use common::real::is_orchestrator_turn_end;
use std::time::Duration;
use tokio::sync::mpsc;
use yhtye_core::acp::{AgentEvent, AgentOutput};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::Role;
use yhtye_core::runtime::Orchestration;

/// ACP tool calls (`title (kind)`) an agent session made, for the log.
fn acp_tools(events: &[ApiEvent], session: &str) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::Agent {
                session: s,
                event: AgentEvent::Output(AgentOutput::ToolCall(tc)),
            } if s == session => Some(format!("{} ({:?})", tc.title, tc.kind)),
            _ => None,
        })
        .collect()
}

/// Sends `request` and waits until the orchestrator has handled `group_settled`.
async fn run_request(
    orch: &Orchestration,
    rx: &mut mpsc::UnboundedReceiver<ApiEvent>,
    request: &str,
) -> Vec<ApiEvent> {
    let mut events = Vec::new();
    common::orch::send(orch, request).await;
    until_healthy(rx, &mut events, REAL_TIMEOUT, |e| {
        matches!(&e.body, ApiEventBody::Prompted { session, text }
            if session == ORCHESTRATOR_SESSION && text.contains("[yhtye:group_settled]"))
    })
    .await;
    until_healthy(rx, &mut events, REAL_TIMEOUT, is_orchestrator_turn_end).await;
    events
}

/// Stage 2 for OpenCode: the orchestrator calls `create_group` / `create_task`,
/// an OpenCode implementer edits README.md and calls `report_step_done`.
#[tokio::test]
#[ignore = "real OpenCode (muse-spark free)"]
async fn real_opencode_orchestrator_creates_task_and_sub_agent_reports_back() {
    let servers_before = opencode_servers();
    let dir = tempfile::tempdir().expect("tempdir");
    let readme = dir.path().join("README.md");
    std::fs::write(&readme, "# demo\n").expect("write README");
    let (orch, mut rx) = Orchestration::start(real_config(dir.path(), opencode_harness(MODEL)))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let events = run_request(
        &orch,
        &mut rx,
        "Create a task that appends the line `hello from yhtye` to README.md.",
    )
    .await;
    let calls = tool_calls(&events);
    eprintln!("tool calls: {calls:#?}");
    eprintln!(
        "orchestrator ACP tools: {:?}",
        acp_tools(&events, ORCHESTRATOR_SESSION)
    );
    eprintln!(
        "orchestrator prompts: {:#?}",
        prompts_to(&events, ORCHESTRATOR_SESSION)
    );
    eprintln!(
        "messages: {:#?}",
        summary(&events)
            .iter()
            .filter(|m| !m.starts_with("Agent") && !m.starts_with("Domain"))
            .collect::<Vec<_>>()
    );
    assert_model(&events);
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
    assert_no_new_servers(&servers_before, Duration::from_secs(10)).await;
}

/// A `code` task with implement → review: the review runs in a separate OpenCode
/// reviewer session and reports a verdict.
#[tokio::test]
#[ignore = "real OpenCode (muse-spark free)"]
async fn real_opencode_implement_then_review() {
    let servers_before = opencode_servers();
    let dir = tempfile::tempdir().expect("tempdir");
    let readme = dir.path().join("README.md");
    std::fs::write(&readme, "# demo\n").expect("write README");
    let (orch, mut rx) = Orchestration::start(real_config(dir.path(), opencode_harness(MODEL)))
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
    let calls = tool_calls(&events);
    eprintln!("tool calls: {calls:#?}");
    assert_model(&events);
    assert!(
        events.iter().any(|e| matches!(
            &e.body,
            ApiEventBody::SessionStarted {
                role: Role::Reviewer,
                ..
            }
        )),
        "a reviewer session was started"
    );
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
    let body = std::fs::read_to_string(&readme).expect("README");
    eprintln!("README.md now: {body:?}");
    assert!(body.contains("reviewed by yhtye"), "{body}");
    shutdown_and_check(orch, &events).await;
    assert_no_new_servers(&servers_before, Duration::from_secs(10)).await;
}
