//! Worktrees of chats, creating branches, and merging a group in its chat's
//! worktree only while that has the base branch checked out
//! (`orchestration-model.md` §6 / §6.1, `core-design.md` §17.4 / §17.8). Real
//! git in temporary repositories.

mod common;

use std::path::{Path, PathBuf};

use common::repo::{TempRepo, git_in, write_in};
use yhtye_core::domain::{GitOp, GitResult, MergeTrigger, TaskKind};
use yhtye_core::git::{
    BranchWorktreeError, CreateBranchError, GitService, list_branches, overview, worktree_root,
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

fn merge_group_into(worktree: &Path, base: &str) -> GitOp {
    GitOp::MergeGroup {
        group: "G-1".into(),
        worktree: worktree.to_path_buf(),
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
async fn a_group_merges_into_its_branch_in_the_chats_worktree() {
    let r = TempRepo::new();
    r.git(&["branch", "release"]);
    group_with_a_file(&r, "release").await;
    let git = r.git_cli();
    let wt = resolve(&r, "release").await;
    let result = git.run(&merge_group_into(&wt, "release")).await;
    assert!(matches!(result, GitResult::Merged { .. }), "{result:?}");
    // The merge happened in the Yhtye worktree of `release`; the main clone
    // is still on `main`, untouched.
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
    let result = git.run(&merge_group_into(&wt, "release")).await;
    assert!(
        matches!(&result, GitResult::Blocked { detail } if detail.contains("README.md")),
        "{result:?}"
    );
    git_in(&wt, &["commit", "-qam", "edit"]);
    let result = git.run(&merge_group_into(&wt, "release")).await;
    assert!(matches!(result, GitResult::Merged { .. }), "{result:?}");
}

/// Merging `G-1` (made on `main`) in the main clone after `change` ran there:
/// it must be blocked without touching anything, with a detail naming `seen`.
async fn blocked_after(change: &[&[&str]], seen: &str) {
    let r = TempRepo::new();
    group_with_a_file(&r, "main").await;
    for args in change {
        r.git(args);
    }
    let head = r.git(&["rev-parse", "HEAD"]);
    let result = r.git_cli().run(&merge_group_into(&r.repo, "main")).await;
    let GitResult::Blocked { detail } = &result else {
        panic!("blocked, not {result:?}");
    };
    assert!(
        detail.contains("Nothing was merged") && detail.contains(seen) && detail.contains("into"),
        "{detail}"
    );
    assert_eq!(r.git(&["rev-parse", "HEAD"]), head, "nothing was merged");
    assert!(!r.repo.join("feature.txt").exists());
}

#[tokio::test]
async fn a_renamed_base_branch_blocks_the_merge_and_names_the_new_branch() {
    blocked_after(&[&["branch", "-m", "main", "trunk"]], "`trunk`").await;
}

#[tokio::test]
async fn another_branch_checked_out_blocks_the_merge() {
    blocked_after(&[&["checkout", "-q", "-b", "other"]], "`other`").await;
}

#[tokio::test]
async fn a_detached_worktree_blocks_the_merge() {
    blocked_after(&[&["checkout", "-q", "--detach"]], "detached HEAD").await;
}

/// Merging `G-1` (made on `main`) in the main clone while someone checks out
/// `other` right after the checks: it must not count as merged into `main`,
/// and the detail must say where the merge went.
async fn switched_during_the_merge(other_at: &str) -> TempRepo {
    let r = TempRepo::new();
    group_with_a_file(&r, "main").await;
    r.git(&["branch", "other", other_at]);
    let main = r.git(&["rev-parse", "main"]);
    let repo = r.repo.clone();
    let git = r
        .git_cli()
        .before_group_merge(move |_| drop(git_in(&repo, &["checkout", "-q", "other"])));
    let result = git.run(&merge_group_into(&r.repo, "main")).await;
    let GitResult::Blocked { detail } = &result else {
        panic!("blocked, not {result:?}");
    };
    assert!(
        detail.contains("`other`") && detail.contains("`main`"),
        "{detail}"
    );
    assert_eq!(r.git(&["rev-parse", "main"]), main, "main is untouched");
    assert!(r.git_status(&["merge-base", "--is-ancestor", "yhtye/G-1", "other"]));
    assert_eq!(
        r.extra_worktrees().len(),
        1,
        "the integration worktree is kept"
    );
    r
}

#[tokio::test]
async fn a_branch_switch_right_before_the_merge_is_reported_with_where_it_went() {
    let r = switched_during_the_merge("main").await;
    let parent = r.git(&["rev-parse", "other^1"]);
    assert_eq!(parent, r.git(&["rev-parse", "main"]), "merged on other");
}

#[tokio::test]
async fn a_switch_to_a_branch_that_has_the_group_is_not_taken_as_merged() {
    // `other` already contains the group branch: `HEAD` has it, `main` does not.
    switched_during_the_merge("yhtye/G-1").await;
}

#[tokio::test]
async fn a_missing_worktree_blocks_the_merge() {
    let r = TempRepo::new();
    r.git(&["branch", "release"]);
    group_with_a_file(&r, "release").await;
    let wt = resolve(&r, "release").await;
    std::fs::remove_dir_all(&wt).expect("rm");
    let result = r.git_cli().run(&merge_group_into(&wt, "release")).await;
    assert!(
        matches!(&result, GitResult::Blocked { detail } if detail.contains("no longer exists")),
        "{result:?}"
    );
    assert!(!wt.exists(), "not recreated");
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
async fn a_directory_reports_the_branch_it_has_checked_out_now() {
    let r = TempRepo::new();
    let git = r.git_cli();
    assert_eq!(git.worktree_branch(&r.repo).await, Ok(Some("main".into())));
    // `git branch -m` moves HEAD of the worktree that has the branch.
    r.git(&["branch", "-m", "main", "trunk"]);
    assert_eq!(git.worktree_branch(&r.repo).await, Ok(Some("trunk".into())));
    // A Yhtye worktree too.
    r.git(&["branch", "feature/x"]);
    let wt = resolve(&r, "feature/x").await;
    git_in(&wt, &["branch", "-m", "feature/y"]);
    assert_eq!(git.worktree_branch(&wt).await, Ok(Some("feature/y".into())));
}

#[tokio::test]
async fn a_tag_named_like_the_branch_does_not_change_the_branch_name() {
    let r = TempRepo::new();
    let git = r.git_cli();
    r.git(&["tag", "main"]);
    assert_eq!(git.current_branch().await, Ok(Some("main".into())));
    assert_eq!(git.worktree_branch(&r.repo).await, Ok(Some("main".into())));
    r.git(&["checkout", "-q", "-b", "yhtye/x"]);
    r.git(&["tag", "yhtye/x"]);
    assert_eq!(
        git.worktree_branch(&r.repo).await,
        Ok(Some("yhtye/x".into()))
    );
}

#[tokio::test]
async fn a_detached_or_missing_directory_has_no_branch() {
    let r = TempRepo::new();
    let git = r.git_cli();
    r.git(&["checkout", "-q", "--detach"]);
    assert_eq!(git.worktree_branch(&r.repo).await, Ok(None));
    assert_eq!(git.worktree_branch(&r.data.join("gone")).await, Ok(None));
}

/// Paths of `git worktree list --porcelain`, as git prints them.
fn listed(r: &TempRepo) -> Vec<String> {
    r.git(&["worktree", "list", "--porcelain"])
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn worktrees_are_given_as_git_lists_them() {
    let r = TempRepo::new();
    let git = r.git_cli();
    r.git(&["branch", "topic"]);
    let topic = git.resolve_branch_worktree("topic").await.expect("topic");
    let made = git.create_branch("fresh", None).await.expect("fresh");
    let main = git.resolve_branch_worktree("main").await.expect("main");
    let shown = |p: &Path| p.to_string_lossy().into_owned();
    let all = listed(&r);
    for p in [&topic, &made, &main] {
        assert!(all.contains(&shown(p)), "{p:?} in {all:?}");
    }
}

#[tokio::test]
async fn find_worktree_knows_only_existing_worktrees_of_the_repository() {
    let r = TempRepo::new();
    let git = r.git_cli();
    let main = git.find_worktree(&r.repo).await.expect("git");
    assert_eq!(main.map(|p| canonical(&p)), Some(canonical(&r.repo)));
    let other = r.data.join("not-a-worktree");
    std::fs::create_dir_all(&other).expect("mkdir");
    assert_eq!(git.find_worktree(&other).await, Ok(None));
    assert_eq!(git.find_worktree(&r.repo.join("sub")).await, Ok(None));
    r.git(&["branch", "topic"]);
    let topic = resolve(&r, "topic").await;
    std::fs::remove_dir_all(&topic).expect("rm");
    assert_eq!(git.find_worktree(&topic).await, Ok(None), "gone");
}

#[tokio::test]
async fn the_overview_lists_every_worktree_main_first() {
    let r = TempRepo::new();
    r.git(&["branch", "topic"]);
    let topic = resolve(&r, "topic").await;
    r.git(&["branch", "gone"]);
    let gone = resolve(&r, "gone").await;
    std::fs::remove_dir_all(&gone).expect("rm");
    git_in(&topic, &["checkout", "-q", "--detach"]);
    let o = overview(&r.repo, 10).await.expect("overview");
    let rows: Vec<(PathBuf, Option<&str>, bool, bool, bool)> = o
        .worktrees
        .iter()
        .map(|w| {
            let p = Path::new(&w.path);
            let p = if p.exists() {
                canonical(p)
            } else {
                p.to_path_buf()
            };
            (
                p,
                w.branch.as_deref(),
                w.head_sha.is_some(),
                w.is_main,
                w.missing,
            )
        })
        .collect();
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert_eq!(
        rows[0],
        (canonical(&r.repo), Some("main"), true, true, false)
    );
    assert!(
        rows.contains(&(topic, None, true, false, false)),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|(p, b, _, main, missing)| p.ends_with("gone")
                && *b == Some("gone")
                && !main
                && *missing),
        "{rows:?}"
    );
}
