//! Worktrees Yhtye manages outside the repository (`orchestration-model.md` §6).
//! Creating one that already exists on the right branch is a no-op, so an
//! operation cut short by a restart can simply run again.

use std::path::{Path, PathBuf};

use super::run::{git, git_ok};

/// `branch` checked out at `path`. Reuses a worktree already there on that
/// branch; refuses to touch anything else found at `path`.
pub(super) async fn ensure_worktree(repo: &Path, path: &Path, branch: &str) -> Result<(), String> {
    if !path.exists() && is_registered(repo, path).await? {
        // Deleted behind git's back: drop just this registration (not a global
        // `worktree prune`, which could touch the user's own worktrees).
        let shown = path.to_string_lossy();
        git_ok(repo, &["worktree", "remove", "--force", &shown]).await?;
    }
    if path.exists() {
        return match worktree_branch(repo, path).await? {
            Some(b) if b == branch => Ok(()),
            Some(other) => Err(format!(
                "{} is a worktree of {other}, not {branch}",
                path.display()
            )),
            None => Err(format!(
                "{} exists but is not a worktree of this repository; move it away",
                path.display()
            )),
        };
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let shown = path.to_string_lossy();
    git_ok(repo, &["worktree", "add", "-q", &shown, branch])
        .await
        .map(|_| ())
}

/// Removes the worktree at `path` (a no-op if there is none). Ignored files
/// such as build output go with it; callers commit real work first.
pub(super) async fn remove_worktree(repo: &Path, path: &Path) -> Result<(), String> {
    if is_registered(repo, path).await? {
        let shown = path.to_string_lossy();
        git_ok(repo, &["worktree", "remove", "--force", &shown]).await?;
    }
    // The group directory goes once its last worktree is gone (fails if not empty).
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::remove_dir(parent).await;
    }
    Ok(())
}

/// Whether `path` is an existing worktree of the repository.
pub(super) async fn is_worktree(repo: &Path, path: &Path) -> Result<bool, String> {
    Ok(path.exists() && is_registered(repo, path).await?)
}

/// Whether git has a worktree registered at `path` (its directory may be gone).
pub(super) async fn is_registered(repo: &Path, path: &Path) -> Result<bool, String> {
    Ok(list(repo).await?.iter().any(|w| same_path(&w.path, path)))
}

/// Branch checked out in the worktree at `path` (`None`: not a worktree, or a
/// detached one).
async fn worktree_branch(repo: &Path, path: &Path) -> Result<Option<String>, String> {
    Ok(list(repo)
        .await?
        .into_iter()
        .find(|w| same_path(&w.path, path))
        .and_then(|w| w.branch))
}

pub(super) struct Worktree {
    pub(super) path: PathBuf,
    pub(super) branch: Option<String>,
    /// Locked with `git worktree lock` (git never removes or prunes it).
    pub(super) locked: bool,
    /// Git would prune it: its directory is gone.
    pub(super) prunable: bool,
}

/// Whether `path` is inside `dir` (symlinks resolved as far as they exist).
pub(super) fn is_under(path: &Path, dir: &Path) -> bool {
    normalize(path).starts_with(normalize(dir))
}

/// `git worktree list --porcelain`.
pub(super) async fn list(repo: &Path) -> Result<Vec<Worktree>, String> {
    let out = git(repo, &["worktree", "list", "--porcelain"]).await?;
    let text = out.into_result("git worktree list")?;
    let mut all = Vec::new();
    for block in text.split("\n\n") {
        let mut path = None;
        let mut branch = None;
        let (mut locked, mut prunable) = (false, false);
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_string());
            } else if line == "locked" || line.starts_with("locked ") {
                locked = true;
            } else if line == "prunable" || line.starts_with("prunable ") {
                prunable = true;
            }
        }
        if let Some(path) = path {
            all.push(Worktree {
                path,
                branch,
                locked,
                prunable,
            });
        }
    }
    Ok(all)
}

fn same_path(a: &Path, b: &Path) -> bool {
    normalize(a) == normalize(b)
}

/// `path` with symlinks resolved as far as it exists (a deleted worktree
/// directory is resolved through its parent).
fn normalize(path: &Path) -> PathBuf {
    if let Ok(p) = std::fs::canonicalize(path) {
        return p;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => normalize(parent).join(name),
        _ => path.to_path_buf(),
    }
}
