//! Orchestrations with fake agents on a real (temporary) git repository and
//! [`GitCli`](yhtye_core::git::GitCli): task worktrees and branches, merges into
//! the group branch, conflicts raised as help, the group merge into the base
//! branch, a dirty base tree, and a restart whose new session is shown the diff.

mod common;

use common::orch::ORCHESTRATOR_SESSION;

use std::time::Duration;

use common::fake_harness;
use common::orch::{
    assert_persisted, domain_events, git_config, is_message, messages, prompts_to,
    shutdown_and_check, until,
};
use common::repo::TempRepo;
use serde_json::{Value, json};
use yhtye_core::api::ApiEvent;
use yhtye_core::domain::{DomainEvent, GroupStatus, HelpKind};
use yhtye_core::runtime::{Orchestration, OrchestrationConfig};

const TIMEOUT: Duration = Duration::from_secs(30);

fn task(title: &str, steps: Value, instruction: &str) -> Value {
    json!({"mcp_call": {"tool": "create_task", "args": {
        "group_id": "${group_id}", "title": title, "kind": "code",
        "steps": steps, "instruction": instruction}}})
}

fn plan(tasks: Vec<Value>) -> Value {
    let mut actions =
        vec![json!({"mcp_call": {"tool": "create_group", "args": {"title": "work"}}})];
    actions.extend(tasks);
    actions.push(json!({"message": "planned"}));
    json!({"match": "[yhtye:user_message]", "actions": actions})
}

fn finish_turn() -> Value {
    json!({"match": "[yhtye:group_settled]", "actions": [
        {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "done"}}}
    ]})
}

/// An implementer turn for prompts containing `needle`: write a file, report.
fn write_turn(needle: &str, path: &str, text: &str) -> Value {
    json!({"match": needle, "actions": [
        {"write_file": {"path": path, "text": text}},
        {"mcp_call": {"tool": "report_step_done", "args": {"result": format!("wrote {path}")}}}
    ]})
}

fn finished(e: &ApiEvent) -> bool {
    is_message(e, ORCHESTRATOR_SESSION, "mcp:finish_group:")
}

async fn run(cfg: OrchestrationConfig) -> (Orchestration, Vec<ApiEvent>) {
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    common::orch::send(&orch, "do the work").await;
    until(&mut rx, &mut events, TIMEOUT, finished).await;
    (orch, events)
}

fn finish_reply(events: &[ApiEvent]) -> String {
    messages(events, ORCHESTRATOR_SESSION)
        .into_iter()
        .find(|m| m.starts_with("mcp:finish_group:"))
        .expect("finish_group reply")
}

async fn assert_clean_finish(r: &TempRepo, orch: Orchestration, events: &[ApiEvent]) {
    assert_eq!(r.status(), "", "the main worktree is clean");
    assert!(r.extra_worktrees().is_empty(), "{:?}", r.extra_worktrees());
    assert_eq!(r.git(&["branch", "--show-current"]).trim(), "main");
    shutdown_and_check(orch, events).await;
}

#[tokio::test]
async fn parallel_code_tasks_merge_and_land_on_the_base_branch() {
    let r = TempRepo::new();
    let implement = json!([{"kind": "implement"}]);
    let reviewed = json!([{"kind": "implement"}, {"kind": "review"}]);
    let orch_script = json!({"turns": [
        plan(vec![task("a", reviewed, "write-a"), task("b", implement, "write-b")]),
        finish_turn()
    ]});
    let impl_script = json!({"turns": [
        write_turn("write-a", "a.txt", "from T-1\n"),
        write_turn("write-b", "b.txt", "from T-2\n")
    ]});
    let reviewer_script = json!({"turns": [{"match": "[yhtye:step]", "actions": [
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "fine", "verdict": "approve"}}}
    ]}]});
    let cfg = git_config(
        &r,
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(reviewer_script),
    );
    let (orch, events) = run(cfg).await;

    assert!(
        finish_reply(&events).contains(r#""ok":true"#),
        "{}",
        finish_reply(&events)
    );
    assert_eq!(r.read("a.txt"), "from T-1\n");
    assert_eq!(r.read("b.txt"), "from T-2\n");
    let review = prompts_to(&events, "T-1/review-1");
    assert_eq!(review.len(), 1, "one review");
    assert!(
        review[0].contains("task branch `yhtye/G-1-T-1`"),
        "{}",
        review[0]
    );
    assert!(review[0].contains("git diff $(git merge-base yhtye/G-1 HEAD)"));
    let subjects = r.git(&["log", "--format=%s", "main"]);
    for needle in [
        "Merge yhtye/G-1 (G-1) into main",
        "Merge yhtye/G-1-T-1 (T-1)",
        "T-2: b",
    ] {
        assert!(subjects.contains(needle), "{needle} in {subjects}");
    }
    assert_persisted(&orch).await;
    assert_clean_finish(&r, orch, &events).await;
}

#[tokio::test]
async fn conflicting_edits_raise_merge_conflict_and_merge_after_the_fix() {
    let r = TempRepo::new();
    let implement = json!([{"kind": "implement"}]);
    let orch_script = json!({"turns": [
        plan(vec![
            task("one", implement.clone(), "edit-readme-1"),
            task("two", implement, "edit-readme-2")
        ]),
        {"match": "[yhtye:help_raised]", "actions": [
            {"mcp_call": {"tool": "modify_steps", "args": {"task_id": "${task}", "steps": [
                {"kind": "implement", "instruction": "resolve-merge"}]}}},
            {"mcp_call": {"tool": "answer_help", "args": {"help_id": "${help_id}", "action": "resume"}}}
        ]},
        finish_turn()
    ]});
    let impl_script = json!({"turns": [
        {"match": "resolve-merge", "actions": [
            {"run": ["git", "merge", "-q", "yhtye/G-1"]},
            {"write_file": {"path": "README.md", "text": "# resolved\n"}},
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "resolved"}}}
        ]},
        write_turn("edit-readme-1", "README.md", "# one\n"),
        write_turn("edit-readme-2", "README.md", "# two\n")
    ]});
    let cfg = git_config(
        &r,
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, events) = run(cfg).await;

    let conflicts: Vec<_> = domain_events(&events)
        .into_iter()
        .filter_map(|e| match e {
            DomainEvent::HelpRaised { help } => Some(help),
            _ => None,
        })
        .collect();
    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
    assert_eq!(conflicts[0].kind, HelpKind::MergeConflict);
    assert!(conflicts[0].message.contains("README.md"));
    assert!(
        messages(&events, &format!("{}/implementer", conflicts[0].task))
            .contains(&"run:1".to_string()),
        "the fix merge conflicted as expected"
    );
    assert!(finish_reply(&events).contains(r#""ok":true"#));
    assert_eq!(r.read("README.md"), "# resolved\n");
    assert_persisted(&orch).await;
    assert_clean_finish(&r, orch, &events).await;
}

#[tokio::test]
async fn a_dirty_base_tree_makes_the_group_merge_blocked() {
    let r = TempRepo::new();
    r.write("README.md", "# the user's unsaved edit\n");
    let orch_script = json!({"turns": [
        plan(vec![task("c", json!([{"kind": "implement"}]), "write-c")]),
        finish_turn()
    ]});
    let impl_script = json!({"turns": [write_turn("write-c", "c.txt", "c\n")]});
    let cfg = git_config(
        &r,
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, events) = run(cfg).await;

    let reply = finish_reply(&events);
    assert!(
        reply.contains("merge_blocked") && reply.contains("README.md"),
        "{reply}"
    );
    assert_eq!(r.read("README.md"), "# the user's unsaved edit\n");
    assert!(!r.repo.join("c.txt").exists());
    assert_eq!(
        r.show("yhtye/G-1", "c.txt"),
        "c\n",
        "the work is on the group branch"
    );
    let snap = orch.snapshot().await.expect("snapshot");
    assert_eq!(
        snap.state.group("G-1").map(|g| g.status),
        Some(GroupStatus::MergeBlocked)
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_blocked_group_merge_is_retried_after_the_user_cleans_the_tree() {
    let r = TempRepo::new();
    r.write("README.md", "# the user's unsaved edit\n");
    let orch_script = json!({"turns": [
        plan(vec![task("c", json!([{"kind": "implement"}]), "write-c")]),
        finish_turn(),
        {"match": "[yhtye:merge_result]", "actions": [{"message": "told the user"}]}
    ]});
    let impl_script = json!({"turns": [write_turn("write-c", "c.txt", "c\n")]});
    let cfg = git_config(
        &r,
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut events = Vec::new();
    common::orch::send(&orch, "do the work").await;
    until(&mut rx, &mut events, TIMEOUT, finished).await;
    assert!(finish_reply(&events).contains("merge_blocked"));

    r.git(&["checkout", "--", "README.md"]);
    let reply = orch.retry_group_merge("G-1").await.expect("retry");
    assert_eq!(reply["merge"]["ok"], json!(true), "{reply}");
    assert_eq!(r.read("c.txt"), "c\n");
    assert!(r.extra_worktrees().is_empty(), "{:?}", r.extra_worktrees());
    until(&mut rx, &mut events, TIMEOUT, |e| {
        is_message(e, ORCHESTRATOR_SESSION, "told the user")
    })
    .await;
    let told = prompts_to(&events, ORCHESTRATOR_SESSION);
    assert!(
        told.iter()
            .any(|p| p.contains("[yhtye:merge_result] group=G-1 ok=true")),
        "{told:?}"
    );
    let err = orch.retry_group_merge("G-1").await.expect_err("done now");
    assert!(err.to_string().contains("merge_blocked"), "{err}");
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_restarted_step_in_a_new_session_is_shown_the_work_already_done() {
    let r = TempRepo::new();
    // First run: the implementer writes part of the work, then Yhtye "quits".
    let orch_script = json!({"turns": [{"match": "[yhtye:user_message]", "actions": [
        {"mcp_call": {"tool": "create_group", "args": {"title": "work"}}},
        task("d", json!([{"kind": "implement"}]), "write-d"),
        "wait_cancel"
    ]}]});
    let impl_script = json!({"turns": [{"match": "write-d", "actions": [
        {"write_file": {"path": "partial.txt", "text": "half done\n"}},
        {"message": "working on it"},
        "wait_cancel"
    ]}]});
    let cfg = git_config(
        &r,
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("starts");
    let mut first = Vec::new();
    common::orch::send(&orch, "do the work").await;
    until(&mut rx, &mut first, TIMEOUT, |e| {
        is_message(e, "T-1/implementer", "working on it")
    })
    .await;
    shutdown_and_check(orch, &first).await;

    // Second run: the harness cannot restore sessions, so the step restarts in
    // a new session whose prompt carries the diff of the worktree.
    let orch_script = json!({"load_session": false, "turns": [finish_turn(),
        {"match": "[yhtye:restarted]", "actions": [{"message": "noted"}]}]});
    let impl_script =
        json!({"load_session": false, "turns": [write_turn("write-d", "rest.txt", "rest\n")]});
    let cfg = git_config(
        &r,
        fake_harness(orch_script),
        fake_harness(impl_script),
        fake_harness(json!({})),
    );
    let (orch, mut rx) = Orchestration::start(cfg).await.expect("restarts");
    let mut events = Vec::new();
    until(&mut rx, &mut events, TIMEOUT, finished).await;

    let prompts = prompts_to(&events, "T-1/implementer");
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    assert!(
        prompts[0].contains("Changes already in the working directory"),
        "{}",
        prompts[0]
    );
    assert!(prompts[0].contains("?? partial.txt"), "{}", prompts[0]);
    assert!(finish_reply(&events).contains(r#""ok":true"#));
    assert_eq!(r.read("partial.txt"), "half done\n");
    assert_eq!(r.read("rest.txt"), "rest\n");
    assert_persisted(&orch).await;
    assert_clean_finish(&r, orch, &events).await;
}
