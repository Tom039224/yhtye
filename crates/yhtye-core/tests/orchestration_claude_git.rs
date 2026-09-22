//! Real Claude Code (always Haiku) on a temporary git repository with the real
//! `GitCli` (Stage 3c): two `code` tasks (one reviewed) in their own worktrees,
//! merged into the group branch, then into the base branch by `finish_group`.
//! Ignored by default:
//! `cargo test -p yhtye-core --test orchestration_claude_git -- --ignored --test-threads=1 --nocapture`.

mod common;

use common::orch::{prompts_to, shutdown_and_check, summary, tool_calls, until};
use common::real::{REAL_TIMEOUT, assert_haiku, is_orchestrator_turn_end, real_git_config};
use common::repo::TempRepo;
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::{GroupStatus, TaskStatus};
use yhtye_core::runtime::{ORCHESTRATOR_SESSION, Orchestration};

const REQUEST: &str = "Create one group with exactly two code tasks. \
Task 1: create add.py defining add(a, b) that returns a + b, plus test_add.py that asserts add(2, 3) == 5; \
steps implement then review. \
Task 2: create sub.py defining sub(a, b) that returns a - b; steps implement only. \
When the group settles, call finish_group.";

fn finish_called(e: &ApiEvent) -> bool {
    matches!(&e.body, ApiEventBody::ToolCalled { record }
        if record.binding.session == ORCHESTRATOR_SESSION && record.tool == "finish_group")
}

fn log_run(events: &[ApiEvent]) {
    eprintln!("tool calls: {:#?}", tool_calls(events));
    eprintln!(
        "reviewer prompts: {:#?}",
        prompts_to(events, "T-1/review-1")
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
async fn real_group_with_two_code_tasks_lands_on_the_base_branch() {
    let r = TempRepo::new();
    let (orch, mut rx) = Orchestration::start(real_git_config(&r))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut events = Vec::new();
    orch.send_user_message(REQUEST).expect("send");
    until(&mut rx, &mut events, REAL_TIMEOUT, finish_called).await;
    until(&mut rx, &mut events, REAL_TIMEOUT, is_orchestrator_turn_end).await;
    log_run(&events);
    assert_haiku(&events);

    let snap = orch.snapshot().await.expect("snapshot");
    let group = snap.state.group("G-1").expect("group G-1");
    eprintln!("group: {group:#?}");
    assert_eq!(group.status, GroupStatus::Done, "{:?}", group.detail);
    let code_tasks: Vec<_> = snap.state.tasks_of("G-1").collect();
    assert!(code_tasks.len() >= 2, "{code_tasks:#?}");
    assert!(
        code_tasks.iter().all(|t| t.status == TaskStatus::Done),
        "{code_tasks:#?}"
    );
    let reviewed = code_tasks
        .iter()
        .any(|t| t.steps.iter().any(|s| s.verdict.is_some()));
    assert!(reviewed, "a review ran: {code_tasks:#?}");

    let log = r.git(&["log", "--format=%s", "main"]);
    eprintln!("main log:\n{log}");
    assert!(log.contains("Merge yhtye/G-1 (G-1) into main"), "{log}");
    for file in ["add.py", "sub.py"] {
        assert!(r.repo.join(file).exists(), "{file} on main");
    }
    eprintln!("add.py:\n{}", r.read("add.py"));
    assert_eq!(r.status(), "", "main worktree clean");
    assert!(r.extra_worktrees().is_empty(), "{:?}", r.extra_worktrees());
    eprintln!("branches:\n{}", r.git(&["branch", "--list"]));
    shutdown_and_check(orch, &events).await;
}

const LONG_COMMAND_REQUEST: &str = "Create one group with exactly one code task with an implement step only. \
The task's instruction must be exactly: first run the shell command `sleep 45`, and only after it has \
finished create the file done.txt containing the word ok. \
When the group settles, call finish_group.";

/// Stage 5 observation: asked to run a long command, the Haiku implementer
/// sometimes ended its turn while the command ran in the background (Claude
/// Code's background tasks), which Yhtye treats as a turn without a report.
/// Stage 6a disables background tasks for the Claude Code harness and tells the
/// sub-agents to wait in the foreground.
#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_implementer_waits_for_a_long_command_before_reporting() {
    let r = TempRepo::new();
    let (orch, mut rx) = Orchestration::start(real_git_config(&r))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut events = Vec::new();
    orch.send_user_message(LONG_COMMAND_REQUEST).expect("send");
    until(&mut rx, &mut events, REAL_TIMEOUT, finish_called).await;
    until(&mut rx, &mut events, REAL_TIMEOUT, is_orchestrator_turn_end).await;
    log_run(&events);
    assert_haiku(&events);

    let snap = orch.snapshot().await.expect("snapshot");
    let tasks: Vec<_> = snap.state.tasks_of("G-1").collect();
    eprintln!("tasks: {tasks:#?}\nhelps: {:#?}", snap.state.helps);
    assert!(
        snap.state.helps.is_empty(),
        "no help (protocol_violation) was raised: {:#?}",
        snap.state.helps
    );
    let t1 = tasks.first().expect("a task");
    assert_eq!(t1.status, TaskStatus::Done);
    assert!(
        t1.steps.iter().all(|s| s.nudges == 0),
        "the implementer reported without a reminder: {:#?}",
        t1.steps
    );
    assert_eq!(r.show("main", "done.txt").trim(), "ok");
    shutdown_and_check(orch, &events).await;
}
