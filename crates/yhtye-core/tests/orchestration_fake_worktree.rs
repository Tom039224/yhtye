//! A chat is bound to its worktree (`orchestration-model.md` §6 / §6.1,
//! Stage 8e): renames and checkouts only change what is shown; right before a
//! group merges, Yhtye checks the branch the worktree is on and returns any
//! mismatch to the orchestrator, which fixes it or merges with `into`. Fake
//! agents, real git.

mod common;

use std::path::Path;

use common::chats::{
    branch_worktree, canonical, cwd_of, is_domain, is_stopped, new_chat, none, repo_config, said,
    say, start, started, wait,
};
use common::orch::{messages, prompts_to, shutdown_and_check};
use common::repo::{TempRepo, git_in};
use serde_json::{Value, json};
use yhtye_core::api::ApiEvent;
use yhtye_core::domain::{DomainEvent, GroupStatus};
use yhtye_core::git::overview;
use yhtye_core::runtime::ChatTarget;

const ORCH: &str = "orchestrator:C-1";

fn merge_finished(ok: bool) -> impl Fn(&ApiEvent) -> bool + Copy {
    move |e| {
        is_domain(
            e,
            |d| matches!(d, DomainEvent::GroupMergeFinished { ok: o, .. } if *o == ok),
        )
    }
}

fn finish_args(into: Option<&str>) -> Value {
    let mut args = json!({"group_id": "G-1", "summary": "done"});
    if let Some(into) = into {
        args["into"] = json!(into);
    }
    json!({"mcp_call": {"tool": "finish_group", "args": args}})
}

/// An orchestrator that plans one task (`work`), calls `finish_group` when the
/// group settles (unless `finishes` is false), and on request calls it again
/// (`again`) or with `into` (`into-<branch>`); the implementer writes `x.txt`
/// slowly, so the test can change the worktree while the group runs.
fn scripts(finishes: bool) -> (Value, Value) {
    let settled = if finishes {
        vec![finish_args(None), json!({"message": "tried"})]
    } else {
        vec![json!({"message": "noted"})]
    };
    let mut turns = vec![
        json!({"match": "work", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "work"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "x", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "write-x"}}},
            {"message": "planned"}
        ]}),
        json!({"match": "[yhtye:group_settled]", "actions": settled}),
        json!({"match": "[yhtye:merge_result]", "actions": [{"message": "got-merge-result"}]}),
        json!({"match": "again", "actions": [finish_args(None), {"message": "tried-again"}]}),
    ];
    for branch in ["renamed", "other", "wrong"] {
        turns.push(json!({"match": format!("into-{branch}"), "actions": [
            finish_args(Some(branch)), {"message": format!("into-{branch}-done")}
        ]}));
    }
    let implementer = json!({"turns": [{"match": "write-x", "actions": [
        {"sleep": 800},
        {"write_file": {"path": "x.txt", "text": "x\n"}},
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "wrote x.txt"}}}
    ]}]});
    (json!({ "turns": turns }), implementer)
}

/// The `finish_group` replies of the orchestrator, oldest first.
fn finish_replies(events: &[ApiEvent]) -> Vec<String> {
    messages(events, ORCH)
        .into_iter()
        .filter(|m| m.starts_with("mcp:finish_group:"))
        .collect()
}

/// The branch the UI shows for the worktree at `dir` (`GitOverview.worktrees`).
async fn shown_branch(repo: &Path, dir: &Path) -> Option<String> {
    let o = overview(repo, 10).await.expect("overview");
    let w = o
        .worktrees
        .into_iter()
        .find(|w| canonical(Path::new(&w.path)) == canonical(dir))
        .expect("the worktree is listed");
    w.branch
}

#[tokio::test]
async fn a_rename_only_changes_the_display_until_the_merge_which_the_orchestrator_finishes_with_into()
 {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let (orch_script, impl_script) = scripts(true);
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let chat = new_chat(&orch, "feature").await;
    let wt = branch_worktree(&r, "feature");
    say(&orch, &chat, "work").await;
    wait(&mut rx, &mut events, said(ORCH, "planned")).await;

    r.git(&["branch", "-m", "feature", "renamed"]);
    assert_eq!(shown_branch(&r.repo, &wt).await.as_deref(), Some("renamed"));
    wait(&mut rx, &mut events, merge_finished(false)).await;
    wait(&mut rx, &mut events, said(ORCH, "tried")).await;

    // Returned to the orchestrator in its finish_group reply; nothing merged.
    let reply = finish_replies(&events).pop().expect("a reply");
    assert!(
        reply.contains("\"ok\":false") && reply.contains("`renamed`") && reply.contains("into"),
        "{reply}"
    );
    let snapshot = orch.snapshot().await.expect("snapshot");
    let g = &snapshot.state.groups[0];
    assert_eq!(
        (g.base_branch.as_str(), g.status),
        ("feature", GroupStatus::MergeBlocked)
    );
    assert!(
        snapshot.state.inbox.is_empty(),
        "no second notice in the inbox"
    );
    assert!(
        !r.git(&["ls-tree", "--name-only", "renamed"])
            .contains("x.txt")
    );

    // `into` must name what is checked out; then the group merges there.
    say(&orch, &chat, "into-wrong").await;
    wait(&mut rx, &mut events, said(ORCH, "into-wrong-done")).await;
    let refused = finish_replies(&events).pop().expect("a reply");
    assert!(
        refused.contains("error")
            && refused.contains("invalid_argument")
            && refused.contains("`renamed`"),
        "{refused}"
    );
    say(&orch, &chat, "into-renamed").await;
    wait(&mut rx, &mut events, merge_finished(true)).await;
    assert_eq!(r.show("renamed", "x.txt"), "x\n");
    assert!(events.iter().any(|e| is_domain(e, |d| matches!(
        d,
        DomainEvent::GroupBaseChanged { from, to, .. } if from == "feature" && to == "renamed"
    ))));
    let g = orch.snapshot().await.expect("snapshot").state.groups[0].clone();
    assert_eq!(
        (g.base_branch.as_str(), g.status),
        ("renamed", GroupStatus::Done)
    );
    assert!(
        !r.branch_exists("feature"),
        "the old name was not recreated"
    );
    assert_eq!(started(&events, ORCH).len(), 1, "never restarted");
    assert_eq!(cwd_of(&events, ORCH), canonical(&wt));
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_checkout_in_the_main_clone_is_returned_the_same_way() {
    let r = TempRepo::new();
    let (orch_script, impl_script) = scripts(true);
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let chat = new_chat(&orch, "main").await;
    say(&orch, &chat, "work").await;
    wait(&mut rx, &mut events, said(ORCH, "planned")).await;

    r.git(&["checkout", "-q", "-b", "other"]);
    wait(&mut rx, &mut events, said(ORCH, "tried")).await;
    let reply = finish_replies(&events).pop().expect("a reply");
    assert!(
        reply.contains("\"ok\":false") && reply.contains("`other`"),
        "{reply}"
    );
    assert_eq!(
        started(&events, ORCH).len(),
        1,
        "the orchestrator stays put"
    );
    assert_eq!(cwd_of(&events, ORCH), canonical(&r.repo));

    say(&orch, &chat, "into-other").await;
    wait(&mut rx, &mut events, merge_finished(true)).await;
    assert_eq!(r.show("other", "x.txt"), "x\n");
    assert!(!r.git(&["ls-tree", "--name-only", "main"]).contains("x.txt"));
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_detached_worktree_is_returned_and_merges_once_the_base_is_checked_out_again() {
    let r = TempRepo::new();
    let (orch_script, impl_script) = scripts(true);
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let chat = new_chat(&orch, "main").await;
    say(&orch, &chat, "work").await;
    wait(&mut rx, &mut events, said(ORCH, "planned")).await;

    r.git(&["checkout", "-q", "--detach"]);
    wait(&mut rx, &mut events, said(ORCH, "tried")).await;
    let reply = finish_replies(&events).pop().expect("a reply");
    assert!(reply.contains("detached HEAD"), "{reply}");

    // Fixed (as the orchestrator would, with its own tools), then finished again.
    r.git(&["checkout", "-q", "main"]);
    say(&orch, &chat, "again").await;
    wait(&mut rx, &mut events, merge_finished(true)).await;
    assert_eq!(r.show("main", "x.txt"), "x\n");
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_group_yhtye_finishes_returns_the_mismatch_to_the_orchestrators_inbox() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let (orch_script, impl_script) = scripts(false);
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let chat = new_chat(&orch, "feature").await;
    say(&orch, &chat, "work").await;
    wait(&mut rx, &mut events, said(ORCH, "planned")).await;
    r.git(&["branch", "-m", "feature", "renamed"]);

    // Reminded once, then finished by Yhtye: blocked, and told as merge_result.
    wait(&mut rx, &mut events, said(ORCH, "got-merge-result")).await;
    let prompts = prompts_to(&events, ORCH);
    let notice = prompts
        .iter()
        .find(|p| p.contains("[yhtye:merge_result]"))
        .expect("a merge_result prompt");
    assert!(
        notice.contains("ok=false")
            && notice.contains("Nothing was merged")
            && notice.contains("`renamed`"),
        "{notice}"
    );
    let g = orch.snapshot().await.expect("snapshot").state.groups[0].clone();
    assert_eq!(g.status, GroupStatus::MergeBlocked);
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_group_cannot_be_created_on_a_detached_worktree() {
    let r = TempRepo::new();
    r.git(&["checkout", "-q", "--detach"]);
    let (orch_script, impl_script) = scripts(true);
    let (orch, mut rx, mut events) = start(repo_config(&r, orch_script, impl_script)).await;
    let chat = orch
        .create_chat_for(ChatTarget::Worktree(r.repo.clone()))
        .await
        .expect("a chat on the detached main clone")
        .id;
    say(&orch, &chat, "work").await;
    wait(&mut rx, &mut events, said(ORCH, "planned")).await;
    let refused = messages(&events, ORCH)
        .into_iter()
        .find(|m| m.starts_with("mcp:create_group:"))
        .expect("create_group reply");
    assert!(
        refused.contains("invalid_state") && refused.contains("detached"),
        "{refused}"
    );
    assert!(
        orch.snapshot()
            .await
            .expect("snapshot")
            .state
            .groups
            .is_empty()
    );
    shutdown_and_check(orch, &events).await;
}

#[tokio::test]
async fn a_process_that_ended_comes_back_in_the_same_worktree_after_a_rename() {
    let r = TempRepo::new();
    r.git(&["branch", "feature"]);
    let script = json!({"turns": [
        {"match": "die", "actions": [{"message": "dying"}, {"crash": 3}]},
        {"match": "again", "actions": [{"message": "back"}]}
    ]});
    let (orch, mut rx, mut events) = start(repo_config(&r, script, none())).await;
    let chat = new_chat(&orch, "feature").await;
    say(&orch, &chat, "die").await;
    wait(&mut rx, &mut events, |e| is_stopped(e, ORCH)).await;
    let wt = branch_worktree(&r, "feature");
    git_in(&wt, &["branch", "-m", "renamed"]);

    say(&orch, &chat, "again").await;
    wait(&mut rx, &mut events, said(ORCH, "back")).await;
    let starts = started(&events, ORCH);
    assert!(starts[1].1, "same worktree: the session was loaded again");
    assert_eq!(cwd_of(&events, ORCH), canonical(&wt));
    shutdown_and_check(orch, &events).await;
}
