//! Real Claude Code (always Haiku): quit Yhtye while a sub-agent is working,
//! start again on the same database, and check that the sub-agent's session is
//! restored with `session/load` and the task completes. Ignored by default:
//! `cargo test -p yhtye-core --test orchestration_claude_restart -- --ignored --test-threads=1 --nocapture`.

mod common;

use common::orch::{
    assert_gapless_from, assert_persisted, prompts_to, shutdown_and_check, summary, tool_calls,
    until,
};
use common::real::{REAL_TIMEOUT, assert_haiku, is_orchestrator_turn_end, real_config};
use yhtye_core::acp::{AgentEvent, AgentOutput};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{DomainEvent, TaskStatus};
use yhtye_core::runtime::{ORCHESTRATOR_SESSION, Orchestration};

const IMPLEMENTER: &str = "T-1/implementer";
const LINE: &str = "hello after restart";

fn started(events: &[ApiEvent], session: &str) -> Vec<(String, bool)> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted {
                session: s,
                acp_session_id,
                resumed,
                ..
            } if s == session => Some((acp_session_id.clone(), *resumed)),
            _ => None,
        })
        .collect()
}

/// Turn output of the implementer (text or a tool call), as opposed to session
/// setup updates such as the available commands.
fn implementer_turn_output(e: &ApiEvent) -> bool {
    matches!(&e.body, ApiEventBody::Agent {
        session,
        event: AgentEvent::Output(
            AgentOutput::MessageChunk(_) | AgentOutput::ThoughtChunk(_) | AgentOutput::ToolCall(_)
        ),
    } if session == IMPLEMENTER)
}

fn prompted_implementer(e: &ApiEvent) -> bool {
    matches!(&e.body, ApiEventBody::Prompted { session, .. } if session == IMPLEMENTER)
}

fn task_statuses(events: &[ApiEvent]) -> Vec<TaskStatus> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::Domain {
                event: DomainEvent::TaskStatusChanged { task, status },
            } if task == "T-1" => Some(*status),
            _ => None,
        })
        .collect()
}

/// First run: plan the task, then "quit" as soon as the implementer is working.
async fn run_until_quit(dir: &std::path::Path) -> Vec<ApiEvent> {
    let (orch, mut rx) = Orchestration::start(real_config(dir))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut events = Vec::new();
    orch.send_user_message(format!(
        "Create a task that appends the line `{LINE}` to README.md."
    ))
    .expect("send");
    // Quit once the implementer is working on its step (mid-turn).
    until(&mut rx, &mut events, REAL_TIMEOUT, prompted_implementer).await;
    until(&mut rx, &mut events, REAL_TIMEOUT, implementer_turn_output).await;
    eprintln!(
        "quitting; T-1 statuses so far: {:?}",
        task_statuses(&events)
    );
    assert_persisted(&orch).await;
    shutdown_and_check(orch, &events).await;
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    events
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_restart_restores_the_sub_agent_session_and_completes_the_task() {
    let dir = tempfile::tempdir().expect("tempdir");
    let readme = dir.path().join("README.md");
    std::fs::write(&readme, "# demo\n").expect("write README");
    let before = run_until_quit(dir.path()).await;
    assert_haiku(&before);
    let old_impl = started(&before, IMPLEMENTER);
    assert_eq!(old_impl.len(), 1, "one implementer session before the quit");
    let reported_before = tool_calls(&before)
        .iter()
        .any(|(s, t, ok)| s == IMPLEMENTER && t == "report_step_done" && *ok);
    assert!(!reported_before, "the quit must happen mid-step");

    // Second run on the same database.
    let (orch, mut rx) = Orchestration::start(real_config(dir.path()))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut events = Vec::new();
    until(&mut rx, &mut events, REAL_TIMEOUT, |e| {
        matches!(&e.body, ApiEventBody::Prompted { session, text }
            if session == ORCHESTRATOR_SESSION && text.contains("[yhtye:group_settled]"))
    })
    .await;
    until(&mut rx, &mut events, REAL_TIMEOUT, is_orchestrator_turn_end).await;

    eprintln!("tool calls after restart: {:#?}", tool_calls(&events));
    eprintln!(
        "implementer sessions: before {old_impl:?}, after {:?}",
        started(&events, IMPLEMENTER)
    );
    eprintln!(
        "orchestrator sessions after: {:?}",
        started(&events, ORCHESTRATOR_SESSION)
    );
    eprintln!(
        "implementer prompts: {:#?}",
        prompts_to(&events, IMPLEMENTER)
    );
    eprintln!(
        "orchestrator prompts: {:#?}",
        prompts_to(&events, ORCHESTRATOR_SESSION)
    );
    eprintln!("T-1 statuses after restart: {:?}", task_statuses(&events));
    let msgs = summary(&events);
    eprintln!(
        "messages: {:#?}",
        msgs.iter()
            .filter(|m| !m.starts_with("Agent") && !m.starts_with("Domain"))
            .collect::<Vec<_>>()
    );

    assert_haiku(&events);
    assert_gapless_from(&events, before.last().map_or(0, |e| e.seq) + 1);
    assert_eq!(
        task_statuses(&events)[..2],
        [TaskStatus::Interrupted, TaskStatus::Running]
    );
    // The implementer's session was restored with session/load (same ACP id).
    assert_eq!(
        started(&events, IMPLEMENTER),
        [(old_impl[0].0.clone(), true)]
    );
    let impl_prompts = prompts_to(&events, IMPLEMENTER);
    assert!(
        impl_prompts[0].starts_with("[yhtye:resume] task=T-1"),
        "{impl_prompts:?}"
    );
    let calls = tool_calls(&events);
    assert!(
        calls.contains(&(IMPLEMENTER.into(), "report_step_done".into(), true)),
        "the restored session reported through the new MCP token: {calls:?}"
    );
    assert!(task_statuses(&events).contains(&TaskStatus::Done));
    let body = std::fs::read_to_string(&readme).expect("README");
    eprintln!("README.md now: {body:?}");
    assert!(body.contains(LINE), "{body}");
    shutdown_and_check(orch, &events).await;
}
