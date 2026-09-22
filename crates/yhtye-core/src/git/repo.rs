//! Repository queries and the few writing commands Yhtye uses (commit, merge,
//! stash). Each function works on one worktree directory.

use std::path::Path;

use super::run::{git, git_env, git_ok};

/// Identity for Yhtye's commits when the repository has none configured.
const FALLBACK_NAME: &str = "Yhtye";
const FALLBACK_EMAIL: &str = "yhtye@localhost";

/// Whether git hooks run for a commit or merge made by Yhtye.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hooks {
    /// Yhtye's internal branches (task commits, task → group merges): hooks and
    /// commit signing are skipped (they may block or prompt).
    Skip,
    /// The user's base branch: the repository's own policy applies.
    Run,
}

/// Outcome of `git merge --no-ff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MergeOutcome {
    /// Merged; the new `HEAD`.
    Merged(String),
    /// Conflicted in these files; the merge was aborted.
    Conflict(Vec<String>),
    Failed(String),
}

pub(super) async fn branch_exists(dir: &Path, branch: &str) -> Result<bool, String> {
    let r = format!("refs/heads/{branch}");
    Ok(git(dir, &["show-ref", "--verify", "--quiet", &r]).await?.ok)
}

/// Commit id of `rev`.
pub(super) async fn rev(dir: &Path, rev: &str) -> Result<String, String> {
    let spec = format!("{rev}^{{commit}}");
    Ok(git_ok(dir, &["rev-parse", "--verify", "-q", &spec])
        .await?
        .trim()
        .to_string())
}

/// Whether `ancestor` is reachable from (or equal to) `rev`.
pub(super) async fn is_ancestor(dir: &Path, ancestor: &str, rev: &str) -> Result<bool, String> {
    Ok(git(dir, &["merge-base", "--is-ancestor", ancestor, rev])
        .await?
        .ok)
}

/// Branch checked out in `dir` (`None`: detached HEAD).
pub(super) async fn current_branch(dir: &Path) -> Result<Option<String>, String> {
    let out = git(dir, &["symbolic-ref", "--short", "-q", "HEAD"]).await?;
    if out.ok {
        return Ok(Some(out.stdout.trim().to_string()));
    }
    // Exit 1 without output: HEAD is detached. Anything else is an error.
    if out.stderr.trim().is_empty() {
        Ok(None)
    } else {
        Err(out.failure("git symbolic-ref HEAD"))
    }
}

/// Paths with changes, untracked files included (`git status --porcelain`).
pub(super) async fn changed_files(dir: &Path) -> Result<Vec<String>, String> {
    status_paths(dir, &["status", "--porcelain", "--untracked-files=all"]).await
}

/// Paths with changes to tracked files (staged or not); untracked files are ignored.
pub(super) async fn tracked_changes(dir: &Path) -> Result<Vec<String>, String> {
    status_paths(dir, &["status", "--porcelain", "--untracked-files=no"]).await
}

async fn status_paths(dir: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let out = git(dir, args).await?;
    if !out.ok {
        return Err(out.failure("git status"));
    }
    Ok(out
        .lines()
        .iter()
        .filter_map(|l| l.get(3..))
        .map(str::to_string)
        .collect())
}

/// Files with unresolved merge conflicts.
pub(super) async fn unmerged_files(dir: &Path) -> Result<Vec<String>, String> {
    let out = git(dir, &["diff", "--name-only", "--diff-filter=U"]).await?;
    if !out.ok {
        return Err(out.failure("git diff --diff-filter=U"));
    }
    Ok(out.lines())
}

/// Unmerged files that still contain conflict markers. An unmerged file whose
/// markers are gone counts as resolved (agents often do not `git add` it).
pub(super) async fn unresolved_files(dir: &Path) -> Result<Vec<String>, String> {
    let mut unresolved = Vec::new();
    for file in unmerged_files(dir).await? {
        let text = tokio::fs::read(dir.join(&file)).await.unwrap_or_default();
        if has_conflict_markers(&String::from_utf8_lossy(&text)) {
            unresolved.push(file);
        }
    }
    Ok(unresolved)
}

fn has_conflict_markers(text: &str) -> bool {
    text.lines()
        .any(|l| l.starts_with("<<<<<<< ") || l.starts_with(">>>>>>> ") || l == "=======")
}

/// Which operation (merge, rebase, ...) is in progress in `dir`, if any.
pub(super) async fn operation_in_progress(dir: &Path) -> Result<Option<&'static str>, String> {
    let heads = [
        ("MERGE_HEAD", "a merge"),
        ("REBASE_HEAD", "a rebase"),
        ("CHERRY_PICK_HEAD", "a cherry-pick"),
        ("REVERT_HEAD", "a revert"),
    ];
    for (head, what) in heads {
        if git(dir, &["rev-parse", "-q", "--verify", head]).await?.ok {
            return Ok(Some(what));
        }
    }
    Ok(None)
}

/// Environment that supplies an identity if the repository has none.
async fn identity_env(dir: &Path) -> Result<Vec<(&'static str, &'static str)>, String> {
    let mut env = Vec::new();
    if !git(dir, &["config", "--get", "user.name"]).await?.ok {
        env.extend([
            ("GIT_AUTHOR_NAME", FALLBACK_NAME),
            ("GIT_COMMITTER_NAME", FALLBACK_NAME),
        ]);
    }
    if !git(dir, &["config", "--get", "user.email"]).await?.ok {
        env.extend([
            ("GIT_AUTHOR_EMAIL", FALLBACK_EMAIL),
            ("GIT_COMMITTER_EMAIL", FALLBACK_EMAIL),
        ]);
    }
    Ok(env)
}

/// Commits everything in `dir` (untracked files included) if anything changed
/// or a merge is waiting to be committed. Returns whether a commit was made.
pub(super) async fn commit_all(dir: &Path, message: &str) -> Result<bool, String> {
    let merging = operation_in_progress(dir).await? == Some("a merge");
    if changed_files(dir).await?.is_empty() && !merging {
        return Ok(false);
    }
    git_ok(dir, &["add", "-A"]).await?;
    let env = identity_env(dir).await?;
    let args = [
        "-c",
        "commit.gpgSign=false",
        "commit",
        "--no-verify",
        "-q",
        "-m",
        message,
    ];
    git_env(dir, &args, &env).await?.into_result("git commit")?;
    Ok(true)
}

/// `git merge --no-ff <branch>` in `dir`. A conflicted merge is aborted, so the
/// worktree is left as it was.
pub(super) async fn merge_no_ff(
    dir: &Path,
    branch: &str,
    message: &str,
    hooks: Hooks,
) -> Result<MergeOutcome, String> {
    let env = identity_env(dir).await?;
    let mut args = vec![];
    if hooks == Hooks::Skip {
        args.extend(["-c", "commit.gpgSign=false"]);
    }
    args.extend(["merge", "--no-ff", "--no-edit", "-m", message]);
    if hooks == Hooks::Skip {
        args.push("--no-verify");
    }
    args.push(branch);
    let out = git_env(dir, &args, &env).await?;
    if out.ok {
        return Ok(MergeOutcome::Merged(rev(dir, "HEAD").await?));
    }
    let conflicted = unmerged_files(dir).await?;
    if operation_in_progress(dir).await? == Some("a merge") {
        git_ok(dir, &["merge", "--abort"]).await?;
    }
    if conflicted.is_empty() {
        Ok(MergeOutcome::Failed(
            out.failure(&format!("git merge {branch}")),
        ))
    } else {
        Ok(MergeOutcome::Conflict(conflicted))
    }
}

/// Moves every change in `dir` (untracked files included) to a new stash entry.
pub(super) async fn stash_all(dir: &Path, message: &str) -> Result<(), String> {
    let env = identity_env(dir).await?;
    let args = ["stash", "push", "--include-untracked", "-m", message];
    git_env(dir, &args, &env)
        .await?
        .into_result("git stash push")
        .map(|_| ())
}

/// `git status --short` plus `git diff <since>` in `dir` (what a restarted agent
/// finds already done), at most `limit` bytes.
pub(super) async fn changes_since(dir: &Path, since: &str, limit: usize) -> Result<String, String> {
    let status = git_ok(dir, &["status", "--short"]).await?;
    let diff = git_ok(dir, &["diff", since]).await?;
    let mut text = format!("$ git status --short\n{status}\n$ git diff {since}\n{diff}");
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n... (truncated)");
    }
    Ok(text)
}

/// `git merge-base <branch> HEAD` in `dir`.
pub(super) async fn git_merge_base(dir: &Path, branch: &str) -> Result<String, String> {
    Ok(git_ok(dir, &["merge-base", branch, "HEAD"])
        .await?
        .trim()
        .to_string())
}
