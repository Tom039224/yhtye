//! A chat keeps up with its branch (`orchestration-model.md` §6.1, Stage 8d):
//! a renamed branch is followed, and the orchestrator is started again where the
//! branch now is checked out. Fake agents, real git.

mod common;

use common::chats::{
    branch_changes, branch_worktree, canonical, cwd_of, is_domain, is_stopped, new_chat, none,
    repo_config, said, say, settle, start, started, wait,
};
use common::orch::{prompts_to, shutdown_and_check};
use common::repo::TempRepo;
use serde_json::{Value, json};
use yhtye_core::domain::{DomainEvent, GroupStatus};

fn merged_ok(e: &yhtye_core::api::ApiEvent) -> bool {
    is_domain(e, |d| {
        matches!(d, DomainEvent::GroupMergeFinished { ok: true, .. })
    })
}

/// An orchestrator that greets, plans one task (`work`), finishes the group,
/// renames its own branch (`rename`) and can be told to die; the implementer
/// writes `x.txt`, after `sleep_ms`.
fn scripts(sleep_ms: u64) -> (Value, Value) {
    let orch = json!({"turns": [
        {"match": "hello", "actions": [{"message": "ready"}]},
        {"match": "work", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "work"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "x", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "write-x"}}},
            {"message": "planned"}
        ]},
        {"match": "rename", "actions": [
            {"run": ["git", "branch", "-m", "feature", "feature-2"]},
            {"message": "renamed-it"}
        ]},
        {"match": "[yhtye:group_settled]", "actions": [
            {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "done"}}}
        ]}
    ]});
    let implementer = json!({"turns": [{"match": "write-x", "actions": [
        {"sleep": sleep_ms},
        {"write_file": {"path": "x.txt", "text": "x\n"}},
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "wrote x.txt"}}}
    ]}]});
    (orch, implementer)
}

#[tokio::test]
async fn an_external_rename_is_followed_and_the_next_group_merges_into_the_new_branch() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let (orch_script, impl_script) = scripts(0);
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let chat = new_chat(&orch, "feature").await;
    say(&orch, &chat, "hello").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "ready")).await;

    r.git(&["branch", "-m", "feature", "renamed"]);
    say(&orch, &chat, "work").await;
    wait(&mut rx, &mut events, merged_ok).await;

    assert_eq!(
        branch_changes(&events),
        [("C-1".into(), "feature".into(), "renamed".into())]
    );
    let snapshot = orch.snapshot().await.expect("snapshot");
    assert_eq!(snapshot.state.chat("C-1").expect("C-1").branch, "renamed");
    assert_eq!(snapshot.state.groups[0].base_branch, "renamed");
    assert_eq!(snapshot.state.groups[0].status, GroupStatus::Done);
    assert_eq!(r.show("renamed", "x.txt"), "x\n");
    assert!(
        !r.branch_exists("feature"),
        "the old name was not recreated"
    );
    // Same worktree: the running orchestrator was not restarted.
    assert_eq!(started(&events, "orchestrator:C-1").len(), 1);
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_rename_by_the_orchestrator_itself_is_seen_when_its_turn_ends() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let (orch_script, impl_script) = scripts(0);
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let chat = new_chat(&orch, "feature").await;
    say(&orch, &chat, "rename").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "renamed-it")).await;
    settle(&mut rx, &mut events).await;
    assert_eq!(
        branch_changes(&events),
        [("C-1".into(), "feature".into(), "feature-2".into())],
        "no further message was needed"
    );
    // The chat's group merges into the new name.
    say(&orch, &chat, "work").await;
    wait(&mut rx, &mut events, merged_ok).await;
    assert_eq!(r.show("feature-2", "x.txt"), "x\n");
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_rename_while_a_group_is_open_moves_its_merge_target() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let (orch_script, impl_script) = scripts(800);
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let chat = new_chat(&orch, "feature").await;
    say(&orch, &chat, "work").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "planned")).await;
    r.git(&["branch", "-m", "feature", "renamed"]);
    wait(&mut rx, &mut events, merged_ok).await;

    assert_eq!(
        branch_changes(&events).len(),
        1,
        "{:?}",
        branch_changes(&events)
    );
    let group = orch.snapshot().await.expect("snapshot").state.groups[0].clone();
    assert_eq!(
        (group.base_branch.as_str(), group.status),
        ("renamed", GroupStatus::Done)
    );
    assert_eq!(r.show("renamed", "x.txt"), "x\n");
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_process_that_ended_is_started_again_after_a_rename() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let script = json!({"turns": [
        {"match": "die", "actions": [{"message": "dying"}, {"crash": 3}]},
        {"match": "again", "actions": [{"message": "back"}]}
    ]});
    let (orch, mut rx, mut events) = start(repo_config(&r, script, none())).await;
    let chat = new_chat(&orch, "feature").await;
    say(&orch, &chat, "die").await;
    wait(&mut rx, &mut events, |e| is_stopped(e, "orchestrator:C-1")).await;
    r.git(&["branch", "-m", "feature", "renamed"]);

    say(&orch, &chat, "again").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "back")).await;
    assert_eq!(
        branch_changes(&events),
        [("C-1".into(), "feature".into(), "renamed".into())]
    );
    let starts = started(&events, "orchestrator:C-1");
    assert!(starts[1].1, "same worktree: the session was loaded again");
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_branch_that_is_gone_without_a_trace_of_a_rename_is_still_refused() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    r.git(&["branch", "never-started"]);
    let script = json!({"turns": [{"match": "hello", "actions": [{"message": "hi"}]}]});
    let (orch, mut rx, mut events) = start(repo_config(&r, script, none())).await;
    let chat = new_chat(&orch, "feature").await;
    let unstarted = new_chat(&orch, "never-started").await;
    say(&orch, &chat, "hello").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "hi")).await;

    // Renamed, but the worktree was left on a detached HEAD: not a rename.
    let wt = branch_worktree(&r, "feature");
    common::repo::git_in(&wt, &["checkout", "-q", "--detach"]);
    r.git(&["branch", "-m", "feature", "renamed"]);
    let err = orch
        .send_user_message(&chat, "x")
        .await
        .expect_err("refused");
    assert!(err.to_string().contains("feature"), "{err}");

    // Renamed before the chat ever ran: nothing says where it went.
    r.git(&["branch", "-m", "never-started", "elsewhere"]);
    let err = orch
        .send_user_message(&unstarted, "x")
        .await
        .expect_err("refused");
    assert!(err.to_string().contains("never-started"), "{err}");
    assert!(branch_changes(&events).is_empty());
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_live_orchestrator_is_started_again_where_its_branch_moved() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let script = json!({"turns": [
        {"match": "one", "actions": [{"message": "ready-one"}]},
        {"match": "two", "actions": [{"message": "ready-two"}]}
    ]});
    let (orch, mut rx, mut events) = start(repo_config(&r, script, none())).await;
    let chat = new_chat(&orch, "feature").await;
    say(&orch, &chat, "one").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "ready-one")).await;
    assert_eq!(
        cwd_of(&events, "orchestrator:C-1"),
        canonical(&branch_worktree(&r, "feature"))
    );

    // The user moves the branch to a worktree of their own.
    r.git(&[
        "worktree",
        "remove",
        "--force",
        &branch_worktree(&r, "feature").to_string_lossy(),
    ]);
    let own = r.data.join("my-feature");
    r.git(&["worktree", "add", "-q", &own.to_string_lossy(), "feature"]);
    say(&orch, &chat, "two").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "ready-two")).await;

    let starts = started(&events, "orchestrator:C-1");
    assert_eq!(starts.len(), 2, "{starts:?}");
    assert!(!starts[1].1, "another directory: a new session");
    assert_eq!(cwd_of(&events, "orchestrator:C-1"), canonical(&own));
    let prompts = prompts_to(&events, "orchestrator:C-1");
    let last = prompts.last().expect("a prompt");
    assert!(
        last.contains("could not be restored") && last.contains("two"),
        "{last}"
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn switching_the_main_clone_to_another_branch_moves_the_orchestrator_of_the_old_one() {
    let r = TempRepo::new();
    let script = json!({"turns": [
        {"match": "one", "actions": [{"message": "ready-one"}]},
        {"match": "two", "actions": [{"message": "ready-two"}]}
    ]});
    let (orch, mut rx, mut events) = start(repo_config(&r, script, none())).await;
    let chat = new_chat(&orch, "main").await;
    say(&orch, &chat, "one").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "ready-one")).await;
    assert_eq!(cwd_of(&events, "orchestrator:C-1"), canonical(&r.repo));

    // `main` is not checked out in the main clone any more: it gets a worktree
    // of its own, and the orchestrator (which can write) starts there.
    r.git(&["checkout", "-q", "-b", "other"]);
    say(&orch, &chat, "two").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "ready-two")).await;
    assert_eq!(
        cwd_of(&events, "orchestrator:C-1"),
        canonical(&branch_worktree(&r, "main"))
    );
    assert!(!started(&events, "orchestrator:C-1")[1].1, "a new session");
    assert_eq!(r.git(&["branch", "--show-current"]).trim(), "other");
    assert!(
        branch_changes(&events).is_empty(),
        "the branch still exists"
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_turn_in_progress_is_not_cut_off_and_the_orchestrator_moves_after_it() {
    let r = TempRepo::new();
    let script = json!({"turns": [
        {"match": "plan", "actions": [
            {"sleep": 1500},
            {"mcp_call": {"tool": "create_group", "args": {"title": "g"}}},
            {"message": "planned-mid"}
        ]},
        {"match": "next", "actions": [{"message": "ready-next"}]}
    ]});
    let (orch, mut rx, mut events) = start(repo_config(&r, script, none())).await;
    let chat = new_chat(&orch, "main").await;
    say(&orch, &chat, "plan").await;
    wait(&mut rx, &mut events, |e| {
        !started(std::slice::from_ref(e), "orchestrator:C-1").is_empty()
    })
    .await;
    // The user switches the main clone away while the orchestrator is thinking.
    r.git(&["checkout", "-q", "-b", "other"]);

    // `create_group` sees the move; the turn still finishes, then the process stops.
    wait(
        &mut rx,
        &mut events,
        said("orchestrator:C-1", "planned-mid"),
    )
    .await;
    wait(&mut rx, &mut events, |e| is_stopped(e, "orchestrator:C-1")).await;
    assert_eq!(started(&events, "orchestrator:C-1").len(), 1);

    say(&orch, &chat, "next").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "ready-next")).await;
    assert_eq!(
        cwd_of(&events, "orchestrator:C-1"),
        canonical(&branch_worktree(&r, "main"))
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_deleted_branch_is_not_mistaken_for_a_rename_when_the_clone_is_on_another_branch() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    r.git(&["checkout", "-q", "feature"]);
    let (orch_script, impl_script) = scripts(0);
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let chat = new_chat(&orch, "feature").await;
    say(&orch, &chat, "hello").await;
    wait(&mut rx, &mut events, said("orchestrator:C-1", "ready")).await;

    // The chat was started in the main clone (which had `feature` checked out).
    // The user goes back to `main` and deletes `feature`: the clone's HEAD names
    // an existing branch, but there is no rename.
    r.git(&["checkout", "-q", "main"]);
    r.git(&["branch", "-D", "feature"]);
    let err = orch
        .send_user_message(&chat, "work")
        .await
        .expect_err("refused");
    assert!(err.to_string().contains("feature"), "{err}");
    assert!(branch_changes(&events).is_empty());
    let snapshot = orch.snapshot().await.expect("snapshot");
    assert_eq!(snapshot.state.chat("C-1").expect("C-1").branch, "feature");
    shutdown_and_check(orch, &events).await;
}
