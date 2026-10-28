//! A finished `code` task is one squash commit on the group branch, a finished
//! group one merge commit on the base branch, both with Yhtye's message and
//! trailers (`orchestration-model.md` §6). `GitCli` against temporary
//! repositories: the commit content, the trailers as git parses them, tasks
//! that were already merged (also after a restart, and by the `--no-ff` merge of
//! an older Yhtye), conflicts and their retry, and the group's message.

mod common;

use std::path::{Path, PathBuf};

use common::repo::{TempRepo, git_in, write_in};
use yhtye_core::agents::AgentChoice;
use yhtye_core::domain::{GitOp, GitResult, GroupTask, MergeTrigger, TaskKind};
use yhtye_core::git::{CREATED_WITH_VERSION, GitService, created_with_line};

const FOOTER: &str = "♪ Created with Yhtye v0.0.0: github.com/Tom039224/yhtye";

fn create_group() -> GitOp {
    GitOp::CreateGroupBranch {
        group: "g1".into(),
        group_branch: "yhtye/g1".into(),
        base_branch: "main".into(),
    }
}

fn prepare(task: &str, kind: TaskKind) -> GitOp {
    GitOp::PrepareWorkspace {
        group: "g1".into(),
        task: task.into(),
        kind,
        group_branch: "yhtye/g1".into(),
        base_branch: "main".into(),
    }
}

fn finish_with(task: &str, agent: Option<AgentChoice>, reviewers: Vec<AgentChoice>) -> GitOp {
    GitOp::FinishTask {
        group: "g1".into(),
        task: task.into(),
        kind: TaskKind::Code,
        message: format!("{task}: work\n\nthe result"),
        group_branch: "yhtye/g1".into(),
        base_branch: "main".into(),
        title: format!("Title of {task}"),
        agent,
        reviewers,
    }
}

fn finish(task: &str) -> GitOp {
    finish_with(task, None, Vec::new())
}

fn merge_group(repo: &Path, tasks: &[(&str, &str)]) -> GitOp {
    GitOp::MergeGroup {
        group: "g1".into(),
        worktree: repo.to_path_buf(),
        group_branch: "yhtye/g1".into(),
        base_branch: "main".into(),
        trigger: MergeTrigger::FinishGroup,
        title: "The group".into(),
        summary: "All done.".into(),
        tasks: tasks
            .iter()
            .map(|(id, title)| GroupTask {
                id: (*id).into(),
                title: (*title).into(),
            })
            .collect(),
    }
}

fn assert_merged(result: &GitResult) {
    assert!(
        matches!(result, GitResult::Merged { .. }),
        "expected merged, got {result:?}"
    );
}

/// A repo with group g1 and a prepared `code` task worktree per name.
async fn with_tasks(tasks: &[&str]) -> (TempRepo, Vec<PathBuf>) {
    let r = TempRepo::new();
    let git = r.git_cli();
    assert_eq!(git.run(&create_group()).await, GitResult::Done);
    let mut dirs = Vec::new();
    for t in tasks {
        match git.run(&prepare(t, TaskKind::Code)).await {
            GitResult::Workspace { path } => dirs.push(path),
            other => panic!("expected a workspace, got {other:?}"),
        }
    }
    (r, dirs)
}

/// The full message of `rev`, without the trailing newline git adds.
fn message(r: &TempRepo, rev: &str) -> String {
    r.git(&["log", "-1", "--format=%B", rev])
        .trim_end()
        .to_string()
}

fn commit_count(r: &TempRepo, range: &str) -> String {
    r.git(&["rev-list", "--count", range]).trim().to_string()
}

fn commit_in(dir: &Path, path: &str, text: &str, subject: &str) {
    write_in(dir, path, text);
    git_in(dir, &["add", "-A"]);
    git_in(dir, &["commit", "-q", "-m", subject]);
}

#[tokio::test]
async fn a_task_is_one_squash_commit_with_title_commits_footer_and_trailers() {
    let (r, dirs) = with_tasks(&["t1"]).await;
    let git = r.git_cli();
    commit_in(&dirs[0], "a.txt", "a\n", "add a");
    commit_in(&dirs[0], "b.txt", "b\n", "add b");
    write_in(&dirs[0], "c.txt", "left uncommitted\n");
    let agent = AgentChoice::new("claude-code", Some("sonnet")).with_effort("high");
    let reviewers = vec![
        AgentChoice::new("codex", Some("gpt-5")),
        AgentChoice::new("grok", None),
    ];
    let result = git.run(&finish_with("t1", Some(agent), reviewers)).await;
    assert_merged(&result);

    assert_eq!(commit_count(&r, "main..yhtye/g1"), "1", "one commit");
    assert_eq!(
        r.git(&["rev-list", "--merges", "--count", "main..yhtye/g1"])
            .trim(),
        "0"
    );
    assert_eq!(
        message(&r, "yhtye/g1"),
        format!(
            "Title of t1\n\
             \n\
             - add a\n\
             - add b\n\
             - t1: work\n\
             \n\
             {FOOTER}\n\
             \n\
             Yhtye-Group: g1\n\
             Yhtye-Task: t1\n\
             Yhtye-Agent: claude-code/sonnet/high\n\
             Yhtye-Reviewer: codex/gpt-5\n\
             Yhtye-Reviewer: grok"
        )
    );
    for file in ["a.txt", "b.txt", "c.txt"] {
        assert!(
            r.git_status(&["cat-file", "-e", &format!("yhtye/g1:{file}")]),
            "{file}"
        );
    }
    assert_eq!(
        commit_count(&r, "main..yhtye/g1--t1"),
        "3",
        "the task branch keeps its commits"
    );
    assert!(!dirs[0].exists(), "task worktree removed");
}

#[tokio::test]
async fn git_reads_the_trailers_of_a_squash_commit() {
    let (r, dirs) = with_tasks(&["t1"]).await;
    write_in(&dirs[0], "a.txt", "a\n");
    let agent = AgentChoice::new("opencode", None);
    let reviewer = AgentChoice::new("claude-code", Some("opus"));
    let op = finish_with("t1", Some(agent), vec![reviewer]);
    assert_merged(&r.git_cli().run(&op).await);

    let trailers = r.git(&["log", "-1", "--format=%(trailers)", "yhtye/g1"]);
    assert_eq!(
        trailers.trim_end(),
        "Yhtye-Group: g1\nYhtye-Task: t1\nYhtye-Agent: opencode\nYhtye-Reviewer: claude-code/opus"
    );
    let body = r.git(&["log", "-1", "--format=%B", "yhtye/g1"]);
    let mut parse = std::process::Command::new("git")
        .args(["interpret-trailers", "--parse"])
        .current_dir(&r.repo)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("git interpret-trailers");
    std::io::Write::write_all(&mut parse.stdin.take().expect("stdin"), body.as_bytes())
        .expect("write");
    let out = parse.wait_with_output().expect("parse");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim_end(),
        trailers.trim_end()
    );
}

#[tokio::test]
async fn a_task_without_agents_or_commits_has_only_the_group_and_task_trailers() {
    let (r, dirs) = with_tasks(&["t1"]).await;
    write_in(&dirs[0], "a.txt", "a\n");
    assert_merged(&r.git_cli().run(&finish("t1")).await);
    let msg = message(&r, "yhtye/g1");
    assert!(
        !msg.contains("Yhtye-Agent") && !msg.contains("Yhtye-Reviewer"),
        "{msg}"
    );
    assert!(msg.ends_with("Yhtye-Group: g1\nYhtye-Task: t1"), "{msg}");
    assert!(msg.contains(created_with_line().as_str()));
    assert_eq!(CREATED_WITH_VERSION, "0.0.0");
    assert_eq!(created_with_line(), FOOTER);
}

#[tokio::test]
async fn the_commits_keep_the_configured_identity() {
    let (r, dirs) = with_tasks(&["t1"]).await;
    write_in(&dirs[0], "a.txt", "a\n");
    assert_merged(&r.git_cli().run(&finish("t1")).await);
    let who = r.git(&["log", "-1", "--format=%an <%ae> / %cn <%ce>", "yhtye/g1"]);
    assert_eq!(
        who.trim(),
        "Test <test@example.com> / Test <test@example.com>"
    );
}

#[tokio::test]
async fn a_task_without_changes_adds_no_commit() {
    let (r, _) = with_tasks(&["t1"]).await;
    let before = r.git(&["rev-parse", "yhtye/g1"]);
    assert_merged(&r.git_cli().run(&finish("t1")).await);
    assert_eq!(r.git(&["rev-parse", "yhtye/g1"]), before);
}

#[tokio::test]
async fn finishing_again_after_a_restart_adds_nothing() {
    let (r, dirs) = with_tasks(&["t1"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "a.txt", "a\n");
    assert_merged(&git.run(&finish("t1")).await);
    let tip = r.git(&["rev-parse", "yhtye/g1"]);

    // Restart after the worktree was removed.
    assert_merged(&git.run(&finish("t1")).await);
    // Restart after the commit but before the worktree was removed.
    let branch = "yhtye/g1--t1";
    git_in(
        &r.repo,
        &["worktree", "add", "-q", &dirs[0].to_string_lossy(), branch],
    );
    assert_merged(&git.run(&finish("t1")).await);
    assert!(!dirs[0].exists(), "the leftover worktree is removed");
    assert_eq!(r.git(&["rev-parse", "yhtye/g1"]), tip, "no second commit");
}

#[tokio::test]
async fn a_squashed_task_stays_merged_after_the_group_changed_the_same_lines() {
    let (r, dirs) = with_tasks(&["t1"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "a.txt", "from the task\n");
    assert_merged(&git.run(&finish("t1")).await);
    // A later task rewrote the file; squashing t1 again would conflict.
    let group_dir = git.group_dir("g1");
    commit_in(&group_dir, "a.txt", "from a later task\n", "later");
    let tip = r.git(&["rev-parse", "yhtye/g1"]);
    assert_merged(&git.run(&finish("t1")).await);
    assert_eq!(r.git(&["rev-parse", "yhtye/g1"]), tip);
    assert_eq!(r.show("yhtye/g1", "a.txt"), "from a later task\n");
}

#[tokio::test]
async fn a_task_merged_with_no_ff_by_an_older_yhtye_counts_as_merged() {
    let (r, dirs) = with_tasks(&["t1"]).await;
    let git = r.git_cli();
    commit_in(&dirs[0], "a.txt", "a\n", "add a");
    let group_dir = git.group_dir("g1");
    git_in(
        &group_dir,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge yhtye/g1--t1 (t1) into yhtye/g1",
            "yhtye/g1--t1",
        ],
    );
    let tip = r.git(&["rev-parse", "yhtye/g1"]);
    assert_merged(&git.run(&finish("t1")).await);
    assert_eq!(r.git(&["rev-parse", "yhtye/g1"]), tip, "no new commit");
    assert!(!dirs[0].exists(), "the worktree is removed");
}

#[tokio::test]
async fn a_conflicting_squash_leaves_the_group_worktree_clean_and_the_retry_works() {
    let (r, dirs) = with_tasks(&["t1", "t2"]).await;
    let git = r.git_cli();
    commit_in(&dirs[0], "README.md", "# from t1\n", "t1 readme");
    commit_in(&dirs[1], "README.md", "# from t2\n", "t2 readme");
    assert_merged(&git.run(&finish("t1")).await);
    let tip = r.git(&["rev-parse", "yhtye/g1"]).trim().to_string();

    let result = git.run(&finish("t2")).await;
    assert_eq!(
        result,
        GitResult::Conflict {
            files: vec!["README.md".into()]
        }
    );
    let group_dir = git.group_dir("g1");
    assert_eq!(
        git_in(&group_dir, &["status", "--porcelain"]),
        "",
        "clean again"
    );
    assert_eq!(
        r.git(&["rev-parse", "yhtye/g1"]).trim(),
        tip,
        "nothing committed"
    );
    let squash_msg = git_in(&group_dir, &["rev-parse", "--git-path", "SQUASH_MSG"]);
    assert!(
        !group_dir.join(squash_msg.trim()).exists(),
        "no squash left behind"
    );
    assert!(dirs[1].exists(), "the task worktree stays for the fix");

    // The agent merges the group branch into the task branch and resolves it.
    let merge = std::process::Command::new("git")
        .args(["merge", "-q", "yhtye/g1"])
        .current_dir(&dirs[1])
        .status()
        .expect("git merge");
    assert!(!merge.success(), "the fix merge conflicts");
    write_in(&dirs[1], "README.md", "# from t1 and t2\n");
    git_in(&dirs[1], &["add", "-A"]);
    git_in(&dirs[1], &["commit", "-q", "--no-edit"]);

    assert_merged(&git.run(&finish("t2")).await);
    assert_eq!(r.show("yhtye/g1", "README.md"), "# from t1 and t2\n");
    assert_eq!(commit_count(&r, &format!("{tip}..yhtye/g1")), "1");
    let msg = message(&r, "yhtye/g1");
    assert!(
        msg.starts_with("Title of t2\n\n- t2 readme\n\n"),
        "only the task's commits: {msg}"
    );
    assert!(!msg.contains("t1 readme"), "{msg}");
    assert!(msg.ends_with("Yhtye-Task: t2"), "{msg}");
}

#[tokio::test]
async fn an_interrupted_squash_is_undone_and_the_task_merged_on_the_retry() {
    let (r, dirs) = with_tasks(&["t1", "t2"]).await;
    let git = r.git_cli();
    commit_in(&dirs[0], "one.txt", "1\n", "t1 work");
    write_in(&dirs[1], "two.txt", "2\n");
    // Yhtye died after staging t1's squash, before committing it.
    let group_dir = git.group_dir("g1");
    git_in(&group_dir, &["merge", "--squash", "yhtye/g1--t1"]);
    assert!(git_in(&group_dir, &["status", "--porcelain"]).contains("one.txt"));

    assert_merged(&git.run(&finish("t2")).await);
    assert_eq!(git_in(&group_dir, &["status", "--porcelain"]), "");
    assert_eq!(commit_count(&r, "main..yhtye/g1"), "1");
    assert!(r.git_status(&["cat-file", "-e", "yhtye/g1:two.txt"]));
    assert!(
        !r.git_status(&["cat-file", "-e", "yhtye/g1:one.txt"]),
        "t1's half-made squash is not part of t2's commit"
    );
    // t1 itself is squashed when it is finished again.
    assert_merged(&git.run(&finish("t1")).await);
    assert_eq!(r.show("yhtye/g1", "one.txt"), "1\n");
    assert_eq!(commit_count(&r, "main..yhtye/g1"), "2");
}

#[tokio::test]
async fn an_interrupted_squash_is_not_saved_as_the_work_of_a_cancelled_group() {
    let (r, dirs) = with_tasks(&["t1"]).await;
    let git = r.git_cli();
    commit_in(&dirs[0], "one.txt", "1\n", "t1 work");
    let group_dir = git.group_dir("g1");
    git_in(&group_dir, &["merge", "--squash", "yhtye/g1--t1"]);
    let tip = r.git(&["rev-parse", "yhtye/g1"]);
    let cancel = GitOp::RemoveGroupWorkspace { group: "g1".into() };
    assert_eq!(git.run(&cancel).await, GitResult::Done);
    assert_eq!(
        r.git(&["rev-parse", "yhtye/g1"]),
        tip,
        "no work-in-progress commit"
    );
    assert!(!group_dir.exists());
}

#[tokio::test]
async fn an_interrupted_squash_does_not_make_an_investigate_task_dirty() {
    let r = TempRepo::new();
    let git = r.git_cli();
    assert_eq!(git.run(&create_group()).await, GitResult::Done);
    let code = match git.run(&prepare("t1", TaskKind::Code)).await {
        GitResult::Workspace { path } => path,
        other => panic!("{other:?}"),
    };
    assert!(matches!(
        git.run(&prepare("look", TaskKind::Investigate)).await,
        GitResult::Workspace { .. }
    ));
    commit_in(&code, "one.txt", "1\n", "t1 work");
    git_in(&git.group_dir("g1"), &["merge", "--squash", "yhtye/g1--t1"]);
    let investigate = GitOp::FinishTask {
        group: "g1".into(),
        task: "look".into(),
        kind: TaskKind::Investigate,
        message: "look".into(),
        group_branch: "yhtye/g1".into(),
        base_branch: "main".into(),
        title: "Look".into(),
        agent: None,
        reviewers: Vec::new(),
    };
    assert_eq!(git.run(&investigate).await, GitResult::Done);
}

#[tokio::test]
async fn dependent_tasks_and_reviews_see_only_their_own_changes_after_a_squash() {
    let (r, dirs) = with_tasks(&["t1", "t2"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "one.txt", "1\n");
    assert_merged(&git.run(&finish("t1")).await);

    // A task that starts now (a dependent one) begins with t1's work.
    let dependent = match git.run(&prepare("t3", TaskKind::Code)).await {
        GitResult::Workspace { path } => path,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        std::fs::read_to_string(dependent.join("one.txt")).expect("one.txt"),
        "1\n"
    );
    write_in(&dependent, "three.txt", "3\n");

    // The review range of the dependent and of t2, which forked before the squash.
    write_in(&dirs[1], "two.txt", "2\n");
    for (dir, own, other) in [
        (&dependent, "three.txt", "one.txt"),
        (&dirs[1], "two.txt", "one.txt"),
    ] {
        let changes = git
            .workspace_changes(dir, "yhtye/g1")
            .await
            .expect("changes");
        assert!(changes.contains(own), "{own} in {changes}");
        assert!(!changes.contains(other), "{other} not in {changes}");
        git_in(dir, &["add", "-A"]);
        git_in(dir, &["commit", "-q", "-m", "work"]);
        let range = git_in(dir, &["merge-base", "yhtye/g1", "HEAD"]);
        let diff = git_in(dir, &["diff", "--name-only", range.trim()]);
        assert_eq!(
            diff.trim(),
            own,
            "the review diff has only the task's own changes"
        );
    }
    assert_merged(&git.run(&finish("t2")).await);
    assert_merged(&git.run(&finish("t3")).await);
    for file in ["one.txt", "two.txt", "three.txt"] {
        assert!(
            r.git_status(&["cat-file", "-e", &format!("yhtye/g1:{file}")]),
            "{file}"
        );
    }
}

#[tokio::test]
async fn the_group_merge_message_lists_the_tasks_in_the_order_they_were_merged() {
    let (r, dirs) = with_tasks(&["t1", "t2"]).await;
    let git = r.git_cli();
    write_in(&dirs[0], "one.txt", "1\n");
    write_in(&dirs[1], "two.txt", "2\n");
    assert_merged(&git.run(&finish("t2")).await);
    assert_merged(&git.run(&finish("t1")).await);

    // The state lists tasks in creation order, and a task that has no commit.
    let op = merge_group(
        &r.repo,
        &[
            ("t1", "Title of t1"),
            ("t2", "Title of t2"),
            ("t3", "No commit"),
        ],
    );
    assert_merged(&git.run(&op).await);

    assert_eq!(
        message(&r, "main"),
        format!(
            "Merge yhtye/g1 into main\n\
             \n\
             The group\n\
             \n\
             All done.\n\
             \n\
             - t2: Title of t2\n\
             - t1: Title of t1\n\
             - t3: No commit\n\
             \n\
             {FOOTER}\n\
             \n\
             Yhtye-Group: g1"
        )
    );
    assert_eq!(
        r.git(&["log", "-1", "--format=%(trailers)", "main"])
            .trim_end(),
        "Yhtye-Group: g1"
    );
    assert_eq!(
        r.git(&["rev-list", "--parents", "-1", "main"])
            .split_whitespace()
            .count(),
        3,
        "a merge commit"
    );
}

#[tokio::test]
async fn the_group_merge_orders_tasks_merged_with_no_ff_by_an_older_yhtye_too() {
    let (r, dirs) = with_tasks(&["t1", "t2"]).await;
    let git = r.git_cli();
    commit_in(&dirs[1], "two.txt", "2\n", "t2 work");
    let group_dir = git.group_dir("g1");
    git_in(
        &group_dir,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge yhtye/g1--t2 (t2) into yhtye/g1",
            "yhtye/g1--t2",
        ],
    );
    write_in(&dirs[0], "one.txt", "1\n");
    assert_merged(&git.run(&finish("t1")).await);

    let op = merge_group(&r.repo, &[("t1", "First"), ("t2", "Second")]);
    assert_merged(&git.run(&op).await);
    let msg = message(&r, "main");
    assert!(msg.contains("- t2: Second\n- t1: First\n"), "{msg}");
}
