//! Working trees of target branches, creating branches, and merging a group
//! where its base branch is checked out (`orchestration-model.md` §6.1,
//! `core-design.md` §17.4). Real git in temporary repositories.

mod common;

use std::path::{Path, PathBuf};

use common::repo::{TempRepo, git_in, write_in};
use yhtye_core::domain::{GitOp, GitResult, MergeTrigger, TaskKind};
use yhtye_core::git::{
    BranchWorktreeError, CreateBranchError, GitService, list_branches, worktree_root,
};

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).expect("canonical path")
}

/// `<data>/worktrees/P-1/branches/<name>` (canonical).
fn managed(r: &TempRepo, name: &str) -> PathBuf {
    worktree_root(&r.data, common::repo::PROJECT)
        .join("branches")
        .join(name)
}

async fn resolve(r: &TempRepo, branch: &str) -> PathBuf {
    let path = r
        .git_cli()
        .resolve_branch_worktree(branch)
        .await
        .unwrap_or_else(|e| panic!("resolve {branch}: {e}"));
    canonical(&path)
}

#[tokio::test]
async fn the_main_clone_is_the_worktree_of_its_branch() {
    let r = TempRepo::new();
    assert_eq!(resolve(&r, "main").await, canonical(&r.repo));
    assert!(r.extra_worktrees().is_empty(), "nothing was created");
}

#[tokio::test]
async fn a_branch_checked_out_nowhere_gets_a_yhtye_worktree_and_nothing_is_switched() {
    let r = TempRepo::new();
    r.git(&["branch", "feature/x"]);
    let path = resolve(&r, "feature/x").await;
    assert_eq!(path, canonical(&managed(&r, "feature-x")));
    assert_eq!(
        git_in(&path, &["branch", "--show-current"]).trim(),
        "feature/x"
    );
    assert_eq!(r.git(&["branch", "--show-current"]).trim(), "main");
    // Resolving again finds the same worktree (idempotent).
    assert_eq!(resolve(&r, "feature/x").await, path);
    assert_eq!(r.extra_worktrees().len(), 1);
}

#[tokio::test]
async fn a_worktree_the_user_made_is_used_as_it_is() {
    let r = TempRepo::new();
    let elsewhere = r.data.join("users-own-worktree");
    r.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "topic",
        &elsewhere.to_string_lossy(),
    ]);
    assert_eq!(resolve(&r, "topic").await, canonical(&elsewhere));
    assert_eq!(r.extra_worktrees().len(), 1, "no second worktree");
}

#[tokio::test]
async fn a_deleted_worktree_directory_is_recreated() {
    let r = TempRepo::new();
    r.git(&["branch", "topic"]);
    let path = resolve(&r, "topic").await;
    std::fs::remove_dir_all(&path).expect("rm");
    let again = resolve(&r, "topic").await;
    assert_eq!(again, path);
    assert!(again.join("README.md").exists());
}

#[tokio::test]
async fn a_detached_main_clone_does_not_matter() {
    let r = TempRepo::new();
    r.git(&["checkout", "-q", "--detach"]);
    r.git(&["branch", "topic"]);
    // `main` is checked out nowhere now: it gets a worktree of its own.
    let main = resolve(&r, "main").await;
    assert_eq!(main, canonical(&managed(&r, "main")));
    assert_eq!(resolve(&r, "topic").await, canonical(&managed(&r, "topic")));
    assert_eq!(
        r.git(&["branch", "--show-current"]).trim(),
        "",
        "still detached"
    );
}

#[tokio::test]
async fn a_missing_branch_is_reported_and_names_never_collide() {
    let r = TempRepo::new();
    let err = r.git_cli().resolve_branch_worktree("nope").await;
    assert_eq!(err, Err(BranchWorktreeError::BranchMissing("nope".into())));
    // `a/b` and `a-b` sanitize to the same directory name.
    r.git(&["branch", "a/b"]);
    r.git(&["branch", "a-b"]);
    let first = resolve(&r, "a/b").await;
    let second = resolve(&r, "a-b").await;
    assert_ne!(first, second);
    assert_eq!(first, canonical(&managed(&r, "a-b")));
    assert_eq!(second, canonical(&managed(&r, "a-b-2")));
}

#[tokio::test]
async fn create_branch_makes_a_worktree_and_leaves_the_main_clone_alone() {
    let r = TempRepo::new();
    let git = r.git_cli();
    let path = git
        .create_branch("feature/new", None)
        .await
        .expect("created");
    assert_eq!(canonical(&path), canonical(&managed(&r, "feature-new")));
    assert_eq!(
        git_in(&path, &["branch", "--show-current"]).trim(),
        "feature/new"
    );
    assert_eq!(r.git(&["branch", "--show-current"]).trim(), "main");
    assert_eq!(r.status(), "");
    assert_eq!(
        r.git(&["rev-parse", "feature/new"]),
        r.git(&["rev-parse", "main"]),
        "at the main clone's HEAD"
    );
    assert_eq!(resolve(&r, "feature/new").await, canonical(&path));
}

#[tokio::test]
async fn create_branch_starts_at_the_given_point_or_the_detached_head() {
    let r = TempRepo::new();
    let git = r.git_cli();
    r.write("second.txt", "2\n");
    r.commit("second");
    let first = r.git(&["rev-parse", "HEAD~1"]);
    git.create_branch("old", Some("HEAD~1")).await.expect("old");
    assert_eq!(r.git(&["rev-parse", "old"]), first);
    r.git(&["checkout", "-q", "--detach", "HEAD~1"]);
    git.create_branch("from-detached", None)
        .await
        .expect("detached");
    assert_eq!(r.git(&["rev-parse", "from-detached"]), first);
}

#[tokio::test]
async fn create_branch_refuses_bad_names_existing_branches_and_missing_starts() {
    let r = TempRepo::new();
    let git = r.git_cli();
    for bad in [
        "",
        "  ",
        "has space",
        "a..b",
        "-x",
        "yhtye/G-9",
        "x.lock",
        "a~b",
        "@{-1}",
    ] {
        let e = git.create_branch(bad, None).await;
        assert!(
            matches!(e, Err(CreateBranchError::InvalidName(_))),
            "{bad:?}: {e:?}"
        );
    }
    assert_eq!(
        git.create_branch("main", None).await,
        Err(CreateBranchError::Exists("main".into()))
    );
    assert_eq!(
        git.create_branch("x", Some("no-such-ref")).await,
        Err(CreateBranchError::FromMissing("no-such-ref".into()))
    );
    assert_eq!(
        git.create_branch("x", Some("--detach")).await,
        Err(CreateBranchError::FromMissing("--detach".into()))
    );
    assert!(!r.branch_exists("x"), "nothing was created");
    assert!(r.extra_worktrees().is_empty());
}

#[tokio::test]
async fn a_start_point_git_cannot_even_look_up_is_a_failure_not_a_missing_one() {
    let r = TempRepo::new();
    // A NUL byte cannot be passed to git at all: not "unknown revision".
    let e = r.git_cli().create_branch("x", Some("a\0b")).await;
    assert!(matches!(e, Err(CreateBranchError::Failed(_))), "{e:?}");
}

#[tokio::test]
async fn list_branches_hides_yhtye_internal_branches() {
    let r = TempRepo::new();
    r.git(&["branch", "yhtye/G-1"]);
    r.git(&["branch", "feature"]);
    let names: Vec<String> = list_branches(&r.repo)
        .await
        .expect("branches")
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(names, ["feature", "main"]);
}

fn merge_group_into(base: &str) -> GitOp {
    GitOp::MergeGroup {
        group: "G-1".into(),
        group_branch: "yhtye/G-1".into(),
        base_branch: base.into(),
        trigger: MergeTrigger::FinishGroup,
    }
}

/// A group `G-1` on `base` with one file merged into its group branch.
async fn group_with_a_file(r: &TempRepo, base: &str) {
    let git = r.git_cli();
    let create = GitOp::CreateGroupBranch {
        group: "G-1".into(),
        group_branch: "yhtye/G-1".into(),
        base_branch: base.into(),
    };
    assert_eq!(git.run(&create).await, GitResult::Done);
    let prepare = GitOp::PrepareWorkspace {
        group: "G-1".into(),
        task: "T-1".into(),
        kind: TaskKind::Code,
        group_branch: "yhtye/G-1".into(),
        base_branch: base.into(),
    };
    let GitResult::Workspace { path } = git.run(&prepare).await else {
        panic!("a workspace");
    };
    write_in(&path, "feature.txt", "feature\n");
    let finish = GitOp::FinishTask {
        group: "G-1".into(),
        task: "T-1".into(),
        kind: TaskKind::Code,
        message: "T-1: work".into(),
        group_branch: "yhtye/G-1".into(),
        base_branch: base.into(),
    };
    assert!(matches!(git.run(&finish).await, GitResult::Merged { .. }));
}

#[tokio::test]
async fn a_group_merges_into_its_branch_in_that_branchs_own_worktree() {
    let r = TempRepo::new();
    r.git(&["branch", "release"]);
    group_with_a_file(&r, "release").await;
    let git = r.git_cli();
    let result = git.run(&merge_group_into("release")).await;
    assert!(matches!(result, GitResult::Merged { .. }), "{result:?}");
    // The merge happened in the Yhtye worktree of `release`; the main clone
    // is still on `main`, untouched.
    let wt = managed(&r, "release");
    assert_eq!(
        std::fs::read_to_string(wt.join("feature.txt")).expect("file"),
        "feature\n"
    );
    assert_eq!(r.git(&["branch", "--show-current"]).trim(), "main");
    assert!(!r.repo.join("feature.txt").exists());
    assert_eq!(r.status(), "");
}

#[tokio::test]
async fn a_dirty_branch_worktree_blocks_the_merge_and_a_clean_one_goes_through() {
    let r = TempRepo::new();
    r.git(&["branch", "release"]);
    group_with_a_file(&r, "release").await;
    let git = r.git_cli();
    let wt = resolve(&r, "release").await;
    write_in(&wt, "README.md", "# edited\n");
    let result = git.run(&merge_group_into("release")).await;
    assert!(
        matches!(&result, GitResult::Blocked { detail } if detail.contains("README.md")),
        "{result:?}"
    );
    git_in(&wt, &["commit", "-qam", "edit"]);
    let result = git.run(&merge_group_into("release")).await;
    assert!(matches!(result, GitResult::Merged { .. }), "{result:?}");
}

#[tokio::test]
async fn merging_into_a_branch_of_a_detached_main_clone_still_works() {
    let r = TempRepo::new();
    group_with_a_file(&r, "main").await;
    r.git(&["checkout", "-q", "--detach"]);
    let result = r.git_cli().run(&merge_group_into("main")).await;
    assert!(matches!(result, GitResult::Merged { .. }), "{result:?}");
    assert!(managed(&r, "main").join("feature.txt").exists());
}

#[tokio::test]
async fn a_deleted_base_branch_blocks_the_merge() {
    let r = TempRepo::new();
    r.git(&["branch", "release"]);
    group_with_a_file(&r, "release").await;
    r.git(&["branch", "-M", "release", "renamed"]);
    let result = r.git_cli().run(&merge_group_into("release")).await;
    assert!(
        matches!(&result, GitResult::Blocked { detail } if detail.contains("release")),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_missing_worktree_of_the_user_is_reported_and_left_registered() {
    let r = TempRepo::new();
    let elsewhere = r.data.join("unmounted-drive");
    r.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "topic",
        &elsewhere.to_string_lossy(),
    ]);
    std::fs::remove_dir_all(&elsewhere).expect("rm");
    let err = r
        .git_cli()
        .resolve_branch_worktree("topic")
        .await
        .expect_err("not resolved");
    assert!(err.to_string().contains("unmounted-drive"), "{err}");
    assert_eq!(r.extra_worktrees().len(), 1, "still registered");
}

#[tokio::test]
async fn a_locked_missing_worktree_of_yhtye_is_left_registered() {
    let r = TempRepo::new();
    r.git(&["branch", "topic"]);
    let path = resolve(&r, "topic").await;
    r.git(&["worktree", "lock", &path.to_string_lossy()]);
    std::fs::remove_dir_all(&path).expect("rm");
    let err = r
        .git_cli()
        .resolve_branch_worktree("topic")
        .await
        .expect_err("not resolved");
    assert!(err.to_string().contains("locked"), "{err}");
    assert_eq!(r.extra_worktrees().len(), 1, "still registered");
}

#[tokio::test]
async fn a_directory_reports_the_branch_it_has_checked_out_and_follows_a_rename() {
    let r = TempRepo::new();
    let git = r.git_cli();
    assert_eq!(git.worktree_branch(&r.repo).await, Ok(Some("main".into())));
    // `git branch -m` moves HEAD of the worktree that has the branch.
    r.git(&["branch", "-m", "main", "trunk"]);
    assert_eq!(git.worktree_branch(&r.repo).await, Ok(Some("trunk".into())));
    assert_eq!(git.branch_exists("main").await, Ok(false));
    // A Yhtye worktree too.
    r.git(&["branch", "feature/x"]);
    let wt = resolve(&r, "feature/x").await;
    git_in(&wt, &["branch", "-m", "feature/y"]);
    assert_eq!(git.worktree_branch(&wt).await, Ok(Some("feature/y".into())));
}

#[tokio::test]
async fn a_detached_or_missing_directory_has_no_branch() {
    let r = TempRepo::new();
    let git = r.git_cli();
    r.git(&["checkout", "-q", "--detach"]);
    assert_eq!(git.worktree_branch(&r.repo).await, Ok(None));
    assert_eq!(git.worktree_branch(&r.data.join("gone")).await, Ok(None));
}

#[tokio::test]
async fn a_rename_is_told_from_a_deletion_by_the_reflog() {
    let r = TempRepo::new();
    let git = r.git_cli();
    r.git(&["branch", "feature"]);
    r.git(&["branch", "-m", "feature", "renamed"]);
    assert_eq!(git.was_renamed("feature", "renamed").await, Ok(true));
    // Not the other way round, and not for an unrelated branch.
    assert_eq!(git.was_renamed("renamed", "feature").await, Ok(false));
    assert_eq!(git.was_renamed("other", "renamed").await, Ok(false));
    // A branch that was only created has no rename in its reflog; nor has a missing one.
    assert_eq!(git.was_renamed("feature", "main").await, Ok(false));
    assert_eq!(git.was_renamed("feature", "nope").await, Ok(false));
}
