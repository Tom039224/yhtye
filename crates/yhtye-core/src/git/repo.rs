//! Repository queries and the few writing commands Yhtye uses (commit, squash,
//! merge, stash). Each function works on one worktree directory.

use std::path::Path;

use super::run::{git, git_env, git_ok};

/// Identity for Yhtye's commits when the repository has none configured.
const FALLBACK_NAME: &str = "Yhtye";
const FALLBACK_EMAIL: &str = "yhtye@localhost";

/// Outcome of `git merge --no-ff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MergeOutcome {
    /// Merged; the new `HEAD`.
    Merged(String),
    /// Conflicted in these files; the merge was aborted.
    Conflict(Vec<String>),
    Failed(String),
}

/// Outcome of `git merge --squash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SquashOutcome {
    /// The branch's changes are staged, waiting to be committed.
    Staged,
    /// The branch adds nothing to `HEAD`.
    Nothing,
    /// Conflicted in these files; the squash was aborted.
    Conflict(Vec<String>),
    Failed(String),
}

/// Separates the commits, and a commit's subject from its trailers, in the
/// output of [`merged_tasks`].
const RECORD: char = '\u{1e}';
const FIELD: char = '\u{1f}';

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

/// Like [`rev`], but `None` if `rev` is not a commit; any other failure is an error.
pub(super) async fn rev_opt(dir: &Path, rev: &str) -> Result<Option<String>, String> {
    let spec = format!("{rev}^{{commit}}");
    let out = git(dir, &["rev-parse", "--verify", "-q", &spec]).await?;
    if out.ok {
        return Ok(Some(out.stdout.trim().to_string()));
    }
    // `--verify -q` exits 1 without a message for an unknown revision.
    if out.stderr.trim().is_empty() {
        Ok(None)
    } else {
        Err(out.failure("git rev-parse"))
    }
}

/// Whether `ancestor` is reachable from (or equal to) `rev`.
pub(super) async fn is_ancestor(dir: &Path, ancestor: &str, rev: &str) -> Result<bool, String> {
    Ok(git(dir, &["merge-base", "--is-ancestor", ancestor, rev])
        .await?
        .ok)
}

/// Branch checked out in `dir` (`None`: detached HEAD, or HEAD pointing
/// outside `refs/heads/`). Not `--short`: that gives `heads/main` when a tag
/// `main` exists too.
pub(super) async fn current_branch(dir: &Path) -> Result<Option<String>, String> {
    let out = git(dir, &["symbolic-ref", "-q", "HEAD"]).await?;
    if out.ok {
        let branch = out.stdout.trim().strip_prefix("refs/heads/");
        return Ok(branch.map(str::to_string));
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
    commit_index(dir, message).await?;
    Ok(true)
}

/// Commits what is staged in `dir` (Yhtye's internal commit: hooks and signing
/// are skipped).
async fn commit_index(dir: &Path, message: &str) -> Result<(), String> {
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
    git_env(dir, &args, &env)
        .await?
        .into_result("git commit")
        .map(|_| ())
}

/// `git merge --no-ff <branch>` in `dir` (the user's base branch: the
/// repository's own hooks and signing policy apply). A conflicted merge is
/// aborted, so the worktree is left as it was.
pub(super) async fn merge_no_ff(
    dir: &Path,
    branch: &str,
    message: &str,
) -> Result<MergeOutcome, String> {
    let env = identity_env(dir).await?;
    let args = ["merge", "--no-ff", "--no-edit", "-m", message, branch];
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

/// `git merge --squash <branch>` in `dir`: stages what `branch` adds, without
/// committing and without a `MERGE_HEAD` (so `git merge --abort` does not apply).
/// A conflicted or failed squash is undone, so the worktree is left as it was.
pub(super) async fn merge_squash(dir: &Path, branch: &str) -> Result<SquashOutcome, String> {
    let out = git(dir, &["merge", "--squash", branch]).await?;
    if out.ok {
        // `git merge --squash` also writes `SQUASH_MSG` when there is nothing to commit.
        if git(dir, &["diff", "--cached", "--quiet"]).await?.ok {
            abort_squash(dir).await?;
            return Ok(SquashOutcome::Nothing);
        }
        return Ok(SquashOutcome::Staged);
    }
    let conflicted = unmerged_files(dir).await?;
    abort_squash(dir).await?;
    if conflicted.is_empty() {
        Ok(SquashOutcome::Failed(
            out.failure(&format!("git merge --squash {branch}")),
        ))
    } else {
        Ok(SquashOutcome::Conflict(conflicted))
    }
}

/// Commits what [`merge_squash`] staged; returns the new `HEAD`.
pub(super) async fn commit_squash(dir: &Path, message: &str) -> Result<String, String> {
    commit_index(dir, message).await?;
    rev(dir, "HEAD").await
}

/// Whether a squash was started in `dir` and not committed (git keeps its
/// message in `SQUASH_MSG` until the commit).
pub(super) async fn squash_in_progress(dir: &Path) -> Result<bool, String> {
    let path = git_ok(dir, &["rev-parse", "--git-path", "SQUASH_MSG"]).await?;
    let file = dir.join(path.trim());
    tokio::fs::try_exists(&file)
        .await
        .map_err(|e| format!("could not look for {}: {e}", file.display()))
}

/// Undoes an uncommitted squash in `dir`, if there is one: the index and the
/// files it changed go back to `HEAD` (a squash has no `MERGE_HEAD`, so `git
/// merge --abort` does not apply). Untracked files stay.
pub(super) async fn abort_squash(dir: &Path) -> Result<(), String> {
    if squash_in_progress(dir).await? {
        git_ok(dir, &["reset", "--merge"]).await?;
    }
    Ok(())
}

/// Subjects of the commits that are not merges in `range`, oldest first.
pub(super) async fn commit_subjects(dir: &Path, range: &str) -> Result<Vec<String>, String> {
    let args = ["log", "--no-merges", "--reverse", "--format=%s", range];
    Ok(git_ok(dir, &args)
        .await?
        .lines()
        .map(str::to_string)
        .collect())
}

/// Ids of the tasks merged into the group branch by the commits of `range`,
/// oldest merge first: from the `Yhtye-Task:` trailer of a squash commit, and
/// from the subject of a `--no-ff` merge of an older Yhtye
/// (`Merge <branch> (<task>) into <group branch>`).
pub(super) async fn merged_tasks(dir: &Path, range: &str) -> Result<Vec<String>, String> {
    let format = "--format=%x1e%s%x1f%(trailers:only,unfold)";
    let args = ["log", "--topo-order", "--reverse", format, range];
    Ok(parse_merged_tasks(&git_ok(dir, &args).await?))
}

fn parse_merged_tasks(log: &str) -> Vec<String> {
    let mut tasks = Vec::new();
    for record in log.split(RECORD).filter(|r| !r.trim().is_empty()) {
        let (subject, trailers) = record.split_once(FIELD).unwrap_or((record, ""));
        let trailed = trailers
            .lines()
            .find_map(|l| l.strip_prefix("Yhtye-Task:"))
            .map(str::trim);
        if let Some(task) = trailed.or_else(|| legacy_merged_task(subject)) {
            tasks.push(task.to_string());
        }
    }
    tasks
}

/// The task of a `Merge <branch> (<task>) into <group branch>` subject.
fn legacy_merged_task(subject: &str) -> Option<&str> {
    let rest = subject.trim().strip_prefix("Merge ")?;
    let (_, rest) = rest.split_once(" (")?;
    let (task, rest) = rest.split_once(") into ")?;
    (!rest.is_empty() && !task.is_empty()).then_some(task)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_tasks_come_from_trailers_and_old_merge_subjects() {
        let log = "\u{1e}Add login\u{1f}Yhtye-Group: api\nYhtye-Task: login\nYhtye-Agent: codex\n\n\
                   \u{1e}Merge yhtye/G-1-T-2 (T-2) into yhtye/G-1\u{1f}\n\n\
                   \u{1e}Unrelated commit\u{1f}\n\n\
                   \u{1e}Merge main into yhtye/g\u{1f}\n\n";
        assert_eq!(parse_merged_tasks(log), ["login", "T-2"]);
    }

    #[test]
    fn only_a_merge_subject_naming_a_task_counts() {
        assert_eq!(
            legacy_merged_task("Merge yhtye/a--b (b) into yhtye/a"),
            Some("b")
        );
        for other in [
            "Merge branch 'main' into yhtye/a",
            "Merge yhtye/a (a) into",
            "Fix (b) into shape",
            "",
        ] {
            assert_eq!(legacy_merged_task(other), None, "{other}");
        }
    }
}
