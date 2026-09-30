//! Working trees of target branches (`orchestration-model.md` §6.1, Stage 8):
//! Orca-style, one worktree per branch, the main clone included. Yhtye never
//! checks a branch out in a worktree that has another one; a branch that is
//! checked out nowhere gets a worktree of its own under `<root>/branches/`.

use std::path::{Path, PathBuf};

use super::repo;
use super::run::{git, git_ok};
use super::worktree::{is_registered, is_under, list};
use super::{BranchWorktreeError, CreateBranchError};
use crate::domain::INTERNAL_BRANCH_PREFIX;

/// Directory (under the worktree root) of the worktrees of target branches.
const BRANCHES_DIR: &str = "branches";

/// The worktree where `branch` is checked out: an existing one (the main clone
/// or any other worktree, Yhtye's or not), else a new one under `<root>/branches/`.
pub(super) async fn resolve(
    repo_dir: &Path,
    root: &Path,
    branch: &str,
) -> Result<PathBuf, BranchWorktreeError> {
    let failed = BranchWorktreeError::Failed;
    if let Some(path) = checked_out(repo_dir, root, branch).await.map_err(failed)? {
        return Ok(path);
    }
    if !repo::branch_exists(repo_dir, branch)
        .await
        .map_err(failed)?
    {
        return Err(BranchWorktreeError::BranchMissing(branch.to_string()));
    }
    let path = free_dir(repo_dir, root, branch).await.map_err(failed)?;
    add_worktree(repo_dir, &path, &["-q", &path.to_string_lossy(), branch])
        .await
        .map_err(failed)?;
    Ok(path)
}

/// Creates `name` at `from` (default: the main clone's HEAD) in a new worktree
/// under `<root>/branches/`. The main clone's checkout does not change.
pub(super) async fn create(
    repo_dir: &Path,
    root: &Path,
    name: &str,
    from: Option<&str>,
) -> Result<PathBuf, CreateBranchError> {
    let failed = CreateBranchError::Failed;
    check_name(repo_dir, name).await?;
    if repo::branch_exists(repo_dir, name).await.map_err(failed)? {
        return Err(CreateBranchError::Exists(name.to_string()));
    }
    let start = match from {
        // A start point that looks like an option is never a branch or commit.
        Some(from) if from.starts_with('-') => {
            return Err(CreateBranchError::FromMissing(from.to_string()));
        }
        Some(from) => match repo::rev_opt(repo_dir, from).await.map_err(failed)? {
            Some(_) => from.to_string(),
            None => return Err(CreateBranchError::FromMissing(from.to_string())),
        },
        None => default_start(repo_dir).await.map_err(failed)?,
    };
    let path = free_dir(repo_dir, root, name).await.map_err(failed)?;
    let shown = path.to_string_lossy();
    let args = ["-q", "--no-track", "-b", name, &shown, &start];
    add_worktree(repo_dir, &path, &args).await.map_err(failed)?;
    Ok(path)
}

/// Removes the worktree of `name` and deletes the branch (the undo of [`create`]).
pub(super) async fn discard(repo_dir: &Path, name: &str) -> Result<(), String> {
    for w in list(repo_dir).await? {
        if w.branch.as_deref() == Some(name) {
            let shown = w.path.to_string_lossy();
            git_ok(repo_dir, &["worktree", "remove", "--force", &shown]).await?;
        }
    }
    git_ok(repo_dir, &["branch", "-D", name]).await.map(|_| ())
}

/// The worktree that has `branch` checked out. A registration whose directory
/// is gone is dropped (just that registration) and does not count, but only for
/// Yhtye's own worktrees (under `root`) that git calls prunable and are not
/// locked. Anyone else's (say, on a drive that is not mounted) is never touched:
/// the error names the path.
async fn checked_out(
    repo_dir: &Path,
    root: &Path,
    branch: &str,
) -> Result<Option<PathBuf>, String> {
    for w in list(repo_dir).await? {
        if w.branch.as_deref() != Some(branch) {
            continue;
        }
        if w.path.exists() {
            return Ok(Some(w.path));
        }
        let shown = w.path.to_string_lossy();
        if !is_under(&w.path, root) {
            return Err(format!(
                "branch {branch} is checked out in {shown}, which does not exist \
                 (an unmounted drive?); restore it or run `git worktree prune`"
            ));
        }
        if w.locked || !w.prunable {
            return Err(format!(
                "the worktree {shown} of branch {branch} is missing but locked; \
                 unlock or remove it with git"
            ));
        }
        git_ok(repo_dir, &["worktree", "remove", "--force", &shown]).await?;
    }
    Ok(None)
}

/// `git worktree add <args>`, after creating the parent directory.
async fn add_worktree(repo_dir: &Path, path: &Path, args: &[&str]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let mut full = vec!["worktree", "add"];
    full.extend_from_slice(args);
    git_ok(repo_dir, &full).await.map(|_| ())
}

/// A directory for a new worktree of `branch`: `<root>/branches/<sanitized>`,
/// with `-2`, `-3`, ... if something (another branch's worktree) is there.
async fn free_dir(repo_dir: &Path, root: &Path, branch: &str) -> Result<PathBuf, String> {
    let base = sanitize(branch);
    for n in 1.. {
        let name = if n == 1 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        let dir = root.join(BRANCHES_DIR).join(name);
        if !dir.exists() && !is_registered(repo_dir, &dir).await? {
            return Ok(dir);
        }
    }
    unreachable!("an unbounded search for a free directory")
}

/// A branch name as a directory name: everything but letters, digits, `.`, `_`
/// and `-` becomes `-` (`feature/x` → `feature-x`).
fn sanitize(branch: &str) -> String {
    let name: String = branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    // A leading dot would hide the directory.
    let name = name.trim_start_matches('.');
    if name.is_empty() { "branch" } else { name }.to_string()
}

async fn check_name(repo_dir: &Path, name: &str) -> Result<(), CreateBranchError> {
    if name.trim().is_empty() {
        return Err(CreateBranchError::InvalidName(
            "the branch name is empty".into(),
        ));
    }
    if name.starts_with(INTERNAL_BRANCH_PREFIX) {
        return Err(CreateBranchError::InvalidName(format!(
            "{INTERNAL_BRANCH_PREFIX}* is reserved for Yhtye's own branches"
        )));
    }
    // A leading `-` would be taken for an option; `--branch` is not used since
    // it also accepts `@{-1}` (a reference to the previous branch).
    let refname = format!("refs/heads/{name}");
    let out = git(repo_dir, &["check-ref-format", &refname])
        .await
        .map_err(CreateBranchError::Failed)?;
    if out.ok && !name.starts_with('-') {
        Ok(())
    } else {
        Err(CreateBranchError::InvalidName(format!(
            "{name} is not a valid branch name"
        )))
    }
}

/// The main clone's branch, or its commit when HEAD is detached.
async fn default_start(repo_dir: &Path) -> Result<String, String> {
    match repo::current_branch(repo_dir).await? {
        Some(branch) => Ok(branch),
        None => repo::rev(repo_dir, "HEAD").await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_names_become_directory_names() {
        assert_eq!(sanitize("feature/x"), "feature-x");
        assert_eq!(sanitize("release-1.2"), "release-1.2");
        assert_eq!(sanitize("a b:c"), "a-b-c");
        assert_eq!(sanitize("..."), "branch");
    }
}
