//! `GitCli` against temporary repositories: worktree lifecycle, merges,
//! conflicts, the user's base tree, and re-running operations after an
//! interrupted run (`orchestration-model.md` §6).

mod common;

use std::path::PathBuf;

use common::repo::{TempRepo, git_in, write_in};
use yhtye_core::domain::{GitOp, GitResult, TaskKind};
use yhtye_core::git::GitService;

fn create_group() -> GitOp {
    GitOp::CreateGroupBranch {
        group: "G-1".into(),
        group_branch: "yhtye/G-1".into(),
        base_branch: "main".into(),
    }
}

fn prepare(task: &str, kind: TaskKind) -> GitOp {
    GitOp::PrepareWorkspace {
        group: "G-1".into(),
        task: task.into(),
        kind,
        group_branch: "yhtye/G-1".into(),
        base_branch: "main".into(),
    }
}

fn finish(task: &str, kind: TaskKind) -> GitOp {
    GitOp::FinishTask {
        group: "G-1".into(),
        task: task.into(),
        kind,
        message: format!("{task}: work\n\nresult"),
        group_branch: "yhtye/G-1".into(),
        base_branch: "main".into(),
    }
}

fn merge_group() -> GitOp {
    GitOp::MergeGroup {
        group: "G-1".into(),
        group_branch: "yhtye/G-1".into(),
        base_branch: "main".into(),
        trigger: yhtye_core::domain::MergeTrigger::FinishGroup,
    }
}

fn remove(task: &str) -> GitOp {
    GitOp::RemoveWorkspace {
        group: "G-1".into(),
        task: task.into(),
    }
}

async fn workspace(git: &impl GitService, op: &GitOp) -> PathBuf {
    match git.run(op).await {
        GitResult::Workspace { path } => path,
        other => panic!("expected a workspace, got {other:?}"),
    }
}

fn assert_merged(result: &GitResult) {
    assert!(
        matches!(result, GitResult::Merged { .. }),
        "expected merged, got {result:?}"
    );
}

/// A repo with group G-1 and a prepared `code` task worktree per name.
async fn with_tasks(tasks: &[&str]) -> (TempRepo, Vec<PathBuf>) {
    let r = TempRepo::new();
    let git = r.git_cli();
    assert_eq!(git.run(&create_group()).await, GitResult::Done);
    let mut dirs = Vec::new();
    for t in tasks {
        dirs.push(workspace(&git, &prepare(t, TaskKind::Code)).await);
    }
    (r, dirs)
}

#[tokio::test]
async fn current_branch_reads_the_main_worktree() {
    let r = TempRepo::new();
    let git = r.git_cli();
    assert_eq!(git.current_branch().await, Ok(Some("main".into())));
    r.git(&["checkout", "-q", "--detach"]);
    assert_eq!(git.current_branch().await, Ok(None));
}

#[tokio::test]
async fn group_branch_and_integration_worktree_are_created_once() {
    let r = TempRepo::new();
    let git = r.git_cli();
    assert_eq!(git.run(&create_group()).await, GitResult::Done);
    assert_eq!(
        r.git(&["rev-parse", "yhtye/G-1"]),
        r.git(&["rev-parse", "main"])
    );
    let group_dir = git.group_dir("G-1");
    assert!(group_dir.join("README.md").exists());
    assert!(
        !group_dir.starts_with(&r.repo),
        "worktrees live outside the repo"
    );
    assert_eq!(
        git.run(&create_group()).await,
        GitResult::Done,
        "re-run is a no-op"
    );
    assert_eq!(r.status(), "", "the user's tree is untouched");
}

#[tokio::test]
async fn an_unknown_existing_group_branch_is_not_adopted() {
    let r = TempRepo::new();
    r.git(&["checkout", "-q", "-b", "yhtye/G-1"]);
    r.write("old.txt", "stale\n");
    r.commit("stale work");
    r.git(&["checkout", "-q", "main"]);
    let result = r.git_cli().run(&create_group()).await;
    assert!(
        matches!(&result, GitResult::Failed { message } if message.contains("already exists")),
        "{result:?}"
    );
}

#[tokio::test]
async fn code_and_investigate_workspaces() {
    let (r, dirs) = with_tasks(&["T-1"]).await;
    let git = r.git_cli();
    assert_eq!(dirs[0], git.task_dir("G-1", "T-1"));
    assert!(r.branch_exists("yhtye/G-1-T-1"));
    assert_eq!(
        git_in(&dirs[0], &["branch", "--show-current"]).trim(),
        "yhtye/G-1-T-1"
    );
    // Preparing again (e.g. after a restart) reuses the worktree.
    write_in(&dirs[0], "wip.txt", "half done\n");
    assert_eq!(
        workspace(&git, &prepare("T-1", TaskKind::Code)).await,
        dirs[0]
    );
    assert!(dirs[0].join("wip.txt").exists(), "work in progress is kept");
    let inv = workspace(&git, &prepare("T-2", TaskKind::Investigate)).await;
    assert_eq!(inv, git.group_dir("G-1"));
}

#[tokio::test]
async fn finishing_commits_leftovers_merges_and_removes_the_worktree() {
    let (r, dirs) = with_tasks(&["T-1"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "src/new.txt", "new file\n");
    let result = git.run(&finish("T-1", TaskKind::Code)).await;
    assert_merged(&result);
    assert_eq!(r.show("yhtye/G-1", "src/new.txt"), "new file\n");
    assert!(!dirs[0].exists(), "task worktree removed");
    assert!(r.branch_exists("yhtye/G-1-T-1"), "task branch kept");
    let subject = r.git(&["log", "-1", "--format=%s", "yhtye/G-1-T-1"]);
    assert_eq!(subject.trim(), "T-1: work");
    // Finishing again after a restart: already merged.
    assert_merged(&git.run(&finish("T-1", TaskKind::Code)).await);
    let merges = r.git(&["rev-list", "--merges", "--count", "yhtye/G-1"]);
    assert_eq!(merges.trim(), "1", "no second merge");
}

#[tokio::test]
async fn parallel_tasks_on_different_files_merge_cleanly() {
    let (r, dirs) = with_tasks(&["T-1", "T-2"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "a.txt", "from T-1\n");
    write_in(&dirs[1], "b.txt", "from T-2\n");
    assert_merged(&git.run(&finish("T-2", TaskKind::Code)).await);
    assert_merged(&git.run(&finish("T-1", TaskKind::Code)).await);
    assert_eq!(r.show("yhtye/G-1", "a.txt"), "from T-1\n");
    assert_eq!(r.show("yhtye/G-1", "b.txt"), "from T-2\n");
}

#[tokio::test]
async fn conflicting_tasks_report_a_conflict_and_merge_after_resolution() {
    let (r, dirs) = with_tasks(&["T-1", "T-2"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "README.md", "# from T-1\n");
    write_in(&dirs[1], "README.md", "# from T-2\n");
    assert_merged(&git.run(&finish("T-1", TaskKind::Code)).await);
    let result = git.run(&finish("T-2", TaskKind::Code)).await;
    assert_eq!(
        result,
        GitResult::Conflict {
            files: vec!["README.md".into()]
        }
    );
    let group_dir = git.group_dir("G-1");
    assert_eq!(
        git_in(&group_dir, &["status", "--porcelain"]),
        "",
        "merge aborted"
    );
    assert!(dirs[1].exists(), "the task worktree stays for the fix");

    // Unresolved conflicts in the task worktree are reported, not committed.
    let merge = std::process::Command::new("git")
        .args(["merge", "-q", "yhtye/G-1"])
        .current_dir(&dirs[1])
        .status()
        .expect("git merge");
    assert!(!merge.success(), "the fix merge conflicts");
    let result = git.run(&finish("T-2", TaskKind::Code)).await;
    assert!(matches!(result, GitResult::Conflict { .. }), "{result:?}");

    // Resolved but not committed: Yhtye completes the merge commit.
    write_in(&dirs[1], "README.md", "# from T-1 and T-2\n");
    assert_merged(&git.run(&finish("T-2", TaskKind::Code)).await);
    assert_eq!(r.show("yhtye/G-1", "README.md"), "# from T-1 and T-2\n");
}

#[tokio::test]
async fn an_interrupted_merge_in_the_integration_worktree_is_aborted_and_retried() {
    let (r, dirs) = with_tasks(&["T-1", "T-2"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "README.md", "# T-1\n");
    write_in(&dirs[1], "other.txt", "T-2\n");
    git_in(&dirs[0], &["commit", "-qam", "t1"]);
    let group_dir = git.group_dir("G-1");
    write_in(&group_dir, "README.md", "# conflicting\n");
    git_in(&group_dir, &["commit", "-qam", "conflicting"]);
    // Leave a conflicted merge behind, as if Yhtye died in the middle of it.
    let _ = std::process::Command::new("git")
        .args(["merge", "-q", "--no-ff", "yhtye/G-1-T-1"])
        .current_dir(&group_dir)
        .status();
    assert_merged(&git.run(&finish("T-2", TaskKind::Code)).await);
    assert_eq!(r.show("yhtye/G-1", "other.txt"), "T-2\n");
}

#[tokio::test]
async fn a_task_without_changes_finishes() {
    let (r, _) = with_tasks(&["T-1"]).await;
    assert_merged(&r.git_cli().run(&finish("T-1", TaskKind::Code)).await);
}

#[tokio::test]
async fn a_dirty_investigate_tree_is_stashed_and_reported_once() {
    let r = TempRepo::new();
    let git = r.git_cli();
    git.run(&create_group()).await;
    let dir = workspace(&git, &prepare("T-1", TaskKind::Investigate)).await;
    assert_eq!(
        git.run(&finish("T-1", TaskKind::Investigate)).await,
        GitResult::Done
    );
    write_in(&dir, "notes.txt", "oops\n");
    let result = git.run(&finish("T-1", TaskKind::Investigate)).await;
    assert_eq!(
        result,
        GitResult::Dirty {
            files: vec!["notes.txt".into()]
        }
    );
    assert!(
        !dir.join("notes.txt").exists(),
        "moved out of the shared tree"
    );
    assert!(r.git(&["stash", "list"]).contains("read-only task T-1"));
    assert_eq!(
        git.run(&finish("T-1", TaskKind::Investigate)).await,
        GitResult::Done
    );
}

#[tokio::test]
async fn removing_a_cancelled_workspace_keeps_its_work_on_the_branch() {
    let (r, dirs) = with_tasks(&["T-1"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "wip.txt", "unfinished\n");
    assert_eq!(git.run(&remove("T-1")).await, GitResult::Done);
    assert!(!dirs[0].exists());
    assert_eq!(r.show("yhtye/G-1-T-1", "wip.txt"), "unfinished\n");
    assert_eq!(
        git.run(&remove("T-1")).await,
        GitResult::Done,
        "re-run is a no-op"
    );
}

#[tokio::test]
async fn removing_a_cancelled_group_keeps_its_branch_and_removes_every_worktree() {
    let (r, dirs) = with_tasks(&["T-1"]).await;
    let git = r.git_cli();
    let group_dir = git.group_dir("G-1");
    write_in(&group_dir, "notes.txt", "left by an investigate task\n");
    assert_eq!(git.run(&remove("T-1")).await, GitResult::Done);
    let op = GitOp::RemoveGroupWorkspace {
        group: "G-1".into(),
    };
    assert_eq!(git.run(&op).await, GitResult::Done);
    assert!(!dirs[0].exists() && !group_dir.exists());
    assert!(r.extra_worktrees().is_empty(), "{:?}", r.extra_worktrees());
    assert_eq!(
        r.show("yhtye/G-1", "notes.txt"),
        "left by an investigate task\n"
    );
    assert_eq!(git.run(&op).await, GitResult::Done, "re-run is a no-op");
    assert_eq!(r.status(), "", "the user's tree is untouched");
}

#[tokio::test]
async fn group_merge_lands_on_the_base_branch_and_cleans_up() {
    let (r, dirs) = with_tasks(&["T-1"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "feature.txt", "feature\n");
    assert_merged(&git.run(&finish("T-1", TaskKind::Code)).await);
    r.write("untracked.txt", "user's scratch file\n");
    let result = git.run(&merge_group()).await;
    assert_merged(&result);
    assert_eq!(r.read("feature.txt"), "feature\n");
    assert_eq!(r.git(&["branch", "--show-current"]).trim(), "main");
    assert!(r.extra_worktrees().is_empty(), "{:?}", r.extra_worktrees());
    assert_eq!(r.read("untracked.txt"), "user's scratch file\n");
    assert_merged(&git.run(&merge_group()).await);
    let merges = r.git(&["rev-list", "--merges", "--count", "main"]);
    assert_eq!(merges.trim(), "2", "the task merge and one group merge");
}

#[tokio::test]
async fn a_dirty_base_tree_blocks_the_merge_and_is_left_alone() {
    let (r, dirs) = with_tasks(&["T-1"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "feature.txt", "feature\n");
    assert_merged(&git.run(&finish("T-1", TaskKind::Code)).await);
    r.write("README.md", "# user's edit\n");
    let result = git.run(&merge_group()).await;
    assert!(
        matches!(&result, GitResult::Blocked { detail } if detail.contains("README.md")),
        "{result:?}"
    );
    assert_eq!(r.read("README.md"), "# user's edit\n");
    assert!(!r.repo.join("feature.txt").exists());
    // Once the user commits, the retry goes through.
    r.commit("user edit");
    assert_merged(&git.run(&merge_group()).await);
    assert_eq!(r.read("feature.txt"), "feature\n");
}

#[tokio::test]
async fn a_conflict_with_the_base_branch_blocks_and_restores_the_tree() {
    let (r, dirs) = with_tasks(&["T-1"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "README.md", "# from the task\n");
    assert_merged(&git.run(&finish("T-1", TaskKind::Code)).await);
    r.write("README.md", "# from the user\n");
    r.commit("user change");
    let result = git.run(&merge_group()).await;
    assert!(
        matches!(&result, GitResult::Blocked { detail } if detail.contains("conflicts in: README.md")),
        "{result:?}"
    );
    assert_eq!(r.status(), "");
    assert_eq!(r.read("README.md"), "# from the user\n");
    assert!(!r.git_status(&["rev-parse", "-q", "--verify", "MERGE_HEAD"]));
}

#[tokio::test]
async fn missing_group_branch_and_worktree_are_recreated() {
    let r = TempRepo::new();
    let git = r.git_cli();
    // As if Yhtye stopped before creating the group branch.
    let dir = workspace(&git, &prepare("T-1", TaskKind::Code)).await;
    assert!(r.branch_exists("yhtye/G-1"));
    // A worktree directory deleted behind git's back is recreated.
    std::fs::remove_dir_all(&dir).expect("rm");
    assert_eq!(workspace(&git, &prepare("T-1", TaskKind::Code)).await, dir);
    assert!(dir.join("README.md").exists());
}

#[tokio::test]
async fn workspace_changes_show_status_and_diff_since_the_fork() {
    let (r, dirs) = with_tasks(&["T-1"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "README.md", "# changed\n");
    write_in(&dirs[0], "new.txt", "new\n");
    let changes = git
        .workspace_changes(&dirs[0], "yhtye/G-1")
        .await
        .expect("changes");
    assert!(changes.contains("?? new.txt"), "{changes}");
    assert!(changes.contains("+# changed"), "{changes}");
    drop(r);
}

#[tokio::test]
async fn an_unfinished_rebase_in_a_task_worktree_is_not_merged_past() {
    let (r, dirs) = with_tasks(&["T-1"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "README.md", "# T-1\n");
    git_in(&dirs[0], &["commit", "-qam", "t1"]);
    let group_dir = git.group_dir("G-1");
    write_in(&group_dir, "README.md", "# group\n");
    git_in(&group_dir, &["commit", "-qam", "group"]);
    let rebase = std::process::Command::new("git")
        .args(["rebase", "-q", "yhtye/G-1"])
        .current_dir(&dirs[0])
        .status()
        .expect("git rebase");
    assert!(!rebase.success(), "the rebase stops on a conflict");
    let result = git.run(&finish("T-1", TaskKind::Code)).await;
    assert!(
        matches!(&result, GitResult::Failed { message } if message.contains("a rebase")),
        "{result:?}"
    );
    assert!(dirs[0].exists(), "the worktree is kept");
}
