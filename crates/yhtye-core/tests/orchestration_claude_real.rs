//! Real Claude Code (always Haiku) through the runtime and state machine. Ignored by default:
//! `cargo test -p yhtye-core --test orchestration_claude_real -- --ignored --test-threads=1 --nocapture`.

mod common;

use common::orch::ORCHESTRATOR_SESSION;

use common::orch::{prompts_to, shutdown_and_check, summary, tool_calls, until};
use common::real::{
    REAL_TIMEOUT, assert_haiku, is_orchestrator_turn_end, real_config, real_git_config,
};
use common::repo::TempRepo;
use tokio::sync::mpsc;
use yhtye_core::acp::{AgentEvent, AgentOutput};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{DomainEvent, GroupStatus, Role, TaskStatus};
use yhtye_core::runtime::Orchestration;

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

/// Sends `request` and waits until the orchestrator has handled `group_settled`.
async fn run_request(
    orch: &Orchestration,
    rx: &mut mpsc::UnboundedReceiver<ApiEvent>,
    request: &str,
) -> Vec<ApiEvent> {
    let mut events = Vec::new();
    common::orch::send(orch, request).await;
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

/// Stage 8d: the orchestrator has all built-in tools. Asked directly, it makes a
/// small change in its own working tree and commits it, without a group.
#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_orchestrator_can_make_and_commit_a_small_change_itself() {
    let r = TempRepo::new();
    r.write("settings.txt", "LIMIT=10\n");
    r.commit("add settings");
    let commits_before = r.git(&["rev-list", "--count", "main"]).trim().to_string();
    let (orch, mut rx) = Orchestration::start(real_git_config(&r))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut events = Vec::new();
    common::orch::send(
        &orch,
        "In settings.txt change LIMIT from 10 to 20. Do it yourself with your own tools \
         (no group, no task) and commit it right away with git.",
    )
    .await;
    // The first turn end is the answer to that message.
    until(&mut rx, &mut events, REAL_TIMEOUT, is_orchestrator_turn_end).await;
    log_run(&events);
    assert_haiku(&events);

    let snap = orch.snapshot().await.expect("snapshot");
    assert!(snap.state.groups.is_empty(), "{:#?}", snap.state.groups);
    let committed = r.show("main", "settings.txt");
    eprintln!("settings.txt on main: {committed:?}");
    assert_eq!(committed.trim(), "LIMIT=20");
    let commits_after = r.git(&["rev-list", "--count", "main"]).trim().to_string();
    assert_ne!(commits_before, commits_after, "a commit was made");
    assert_eq!(r.status(), "", "nothing was left uncommitted");
    assert_eq!(r.git(&["branch", "--show-current"]).trim(), "main");
    shutdown_and_check(orch, &events).await;
}

/// Stage 8d: the test renames the chat's branch behind Yhtye's back. The chat
/// follows it, and the next group is merged into the renamed branch.
#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_chat_follows_a_renamed_branch_and_merges_the_next_group_into_it() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let (orch, mut rx) = Orchestration::start(real_git_config(&r))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut events = Vec::new();
    let chat = orch.create_chat("feature").await.expect("chat").id;
    orch.send_user_message(&chat, "Reply with the single word OK. Do not use any tool.")
        .await
        .expect("send");
    until(&mut rx, &mut events, REAL_TIMEOUT, is_orchestrator_turn_end).await;

    r.git(&["branch", "-m", "feature", "renamed"]);
    orch.send_user_message(
        &chat,
        "Create one group with one code task (steps: implement only) that appends the line \
         `renamed ok` to README.md. When the group settles, call finish_group.",
    )
    .await
    .expect("send");
    let merged = |e: &ApiEvent| {
        matches!(
            &e.body,
            ApiEventBody::Domain {
                event: DomainEvent::GroupMergeFinished { .. }
            }
        )
    };
    until(&mut rx, &mut events, REAL_TIMEOUT, merged).await;
    until(&mut rx, &mut events, REAL_TIMEOUT, is_orchestrator_turn_end).await;
    log_run(&events);
    assert_haiku(&events);

    let changed: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::Domain {
                event: DomainEvent::ChatBranchChanged { chat, from, to },
            } => Some((chat.as_str(), from.as_str(), to.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(changed, [(chat.as_str(), "feature", "renamed")]);
    let snap = orch.snapshot().await.expect("snapshot");
    assert_eq!(snap.state.chat(&chat).expect("chat").branch, "renamed");
    let group = snap.state.group("G-1").expect("group G-1");
    assert_eq!(group.base_branch, "renamed");
    assert_eq!(group.status, GroupStatus::Done, "{:?}", group.detail);
    assert!(r.show("renamed", "README.md").contains("renamed ok"));
    assert!(
        !r.branch_exists("feature"),
        "the old name did not come back"
    );
    assert_eq!(r.git(&["branch", "--show-current"]).trim(), "main");
    shutdown_and_check(orch, &events).await;
}
