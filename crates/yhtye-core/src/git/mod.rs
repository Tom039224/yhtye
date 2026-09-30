//! Git operations behind a trait (`core-design.md` §7): [`GitCli`] runs the
//! `git` CLI (Stage 3c); [`NoopGit`] does nothing (tests without git).

mod branches;
mod cli;
mod graph;
mod repo;
mod run;
mod worktree;

use std::path::{Path, PathBuf};

use async_trait::async_trait;

pub use cli::{GitCli, worktree_root};
pub use graph::{
    GitBranch, GitCommit, GitOverview, GitWorktree, MAX_GRAPH_COMMITS, list_branches, overview,
};

use crate::domain::{GitOp, GitResult};

/// The top-level directory of the work tree containing `dir` (`None`: not in
/// a git work tree). Runs with the same clean environment as every git command.
pub async fn show_toplevel(dir: &Path) -> Result<Option<PathBuf>, String> {
    let out = run::git(dir, &["rev-parse", "--show-toplevel"]).await?;
    let top = out.stdout.trim();
    Ok((out.ok && !top.is_empty()).then(|| PathBuf::from(top)))
}

/// Why a branch has no worktree for a new chat.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BranchWorktreeError {
    /// The branch does not exist.
    #[error("branch {0} does not exist")]
    BranchMissing(String),
    #[error("{0}")]
    Failed(String),
}

/// Why a new branch could not be created.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CreateBranchError {
    /// Not a valid branch name, or a reserved one (`yhtye/*`).
    #[error("{0}")]
    InvalidName(String),
    #[error("branch {0} already exists")]
    Exists(String),
    /// The start point is not a branch or commit.
    #[error("start point {0} does not exist")]
    FromMissing(String),
    #[error("{0}")]
    Failed(String),
}

/// Runs git for the orchestration. Every method is called from the runtime loop
/// and should finish quickly (plain git commands, no network).
#[async_trait]
pub trait GitService: Send + Sync + 'static {
    /// Branch checked out in the project's main worktree (`None`: detached HEAD).
    async fn current_branch(&self) -> Result<Option<String>, String>;

    /// Whether the local branch `branch` exists.
    async fn branch_exists(&self, branch: &str) -> Result<bool, String>;

    /// The branch checked out in the worktree `dir` (`None`: detached HEAD, or
    /// `dir` is gone). A chat's branch is read here when a group is created and
    /// for `finish_group`'s `into` (Stage 8e).
    async fn worktree_branch(&self, dir: &Path) -> Result<Option<String>, String>;

    /// `dir` as `git worktree list` shows it, if it is an existing worktree of
    /// the repository (`None`: not registered, or its directory is gone).
    async fn find_worktree(&self, dir: &Path) -> Result<Option<PathBuf>, String> {
        Ok(Some(dir.to_path_buf()))
    }

    /// The worktree where `branch` is checked out (as `git worktree list` shows
    /// it), creating one under `<worktree root>/branches/` if it is checked out
    /// nowhere (§17.4). Never changes what any worktree has checked out.
    async fn resolve_branch_worktree(&self, branch: &str) -> Result<PathBuf, BranchWorktreeError>;

    /// Creates branch `name` at `from` (default: the main worktree's HEAD) in a
    /// new worktree and returns its directory. The main worktree is not touched.
    async fn create_branch(
        &self,
        name: &str,
        from: Option<&str>,
    ) -> Result<PathBuf, CreateBranchError>;

    /// Undoes [`GitService::create_branch`]: removes the branch's worktree and
    /// deletes the branch (only for a branch Yhtye just created).
    async fn discard_branch(&self, name: &str) -> Result<(), String> {
        let _ = name;
        Ok(())
    }

    /// The highest number `n` of a `yhtye/G-<n>` branch or a `G-<n>` worktree
    /// directory git has (0: none). A new group is numbered past it, so leftovers
    /// of a wiped database do not collide with it.
    async fn highest_group_number(&self) -> Result<u32, String> {
        Ok(0)
    }

    /// Runs `op`. Expected outcomes (conflicts, dirty trees) are results, not errors:
    /// `CreateGroupBranch` / `RemoveWorkspace` → `Done`; `PrepareWorkspace` →
    /// `Workspace`; `FinishTask` → `Merged` / `Conflict` / `Dirty`;
    /// `MergeGroup` → `Merged` / `Blocked` (also when the worktree is not on the
    /// base branch any more, Stage 8e); anything else going wrong → `Failed`.
    async fn run(&self, op: &GitOp) -> GitResult;

    /// `git status` and the diff of `workdir` since its branch forked from
    /// `group_branch` (given to an agent restarted in a new session).
    async fn workspace_changes(
        &self,
        workdir: &Path,
        group_branch: &str,
    ) -> Result<String, String> {
        let _ = (workdir, group_branch);
        Ok(String::new())
    }
}

/// No git at all: every task works in the project directory and every merge
/// "succeeds". Used until the real service exists, and in tests.
#[derive(Debug, Clone)]
pub struct NoopGit {
    project_dir: PathBuf,
    base_branch: Option<String>,
}

impl NoopGit {
    #[must_use]
    pub fn new(project_dir: impl AsRef<Path>, base_branch: Option<String>) -> Self {
        Self {
            project_dir: project_dir.as_ref().to_path_buf(),
            base_branch,
        }
    }
}

#[async_trait]
impl GitService for NoopGit {
    async fn current_branch(&self) -> Result<Option<String>, String> {
        Ok(self.base_branch.clone())
    }

    async fn branch_exists(&self, _branch: &str) -> Result<bool, String> {
        Ok(true)
    }

    async fn worktree_branch(&self, _dir: &Path) -> Result<Option<String>, String> {
        Ok(self.base_branch.clone())
    }

    async fn resolve_branch_worktree(&self, _branch: &str) -> Result<PathBuf, BranchWorktreeError> {
        Ok(self.project_dir.clone())
    }

    async fn create_branch(
        &self,
        _name: &str,
        _from: Option<&str>,
    ) -> Result<PathBuf, CreateBranchError> {
        Ok(self.project_dir.clone())
    }

    async fn run(&self, op: &GitOp) -> GitResult {
        match op {
            GitOp::CreateGroupBranch { .. }
            | GitOp::RemoveWorkspace { .. }
            | GitOp::RemoveGroupWorkspace { .. } => GitResult::Done,
            GitOp::PrepareWorkspace { .. } => GitResult::Workspace {
                path: self.project_dir.clone(),
            },
            GitOp::FinishTask { .. } | GitOp::MergeGroup { .. } => GitResult::Merged {
                detail: "no git in this build (NoopGit)".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TaskKind;

    #[tokio::test]
    async fn noop_git_uses_the_project_dir_and_always_merges() {
        let git = NoopGit::new("/tmp/p", Some("main".into()));
        assert_eq!(git.current_branch().await, Ok(Some("main".into())));
        let ws = git
            .run(&GitOp::PrepareWorkspace {
                group: "G-1".into(),
                task: "T-1".into(),
                kind: TaskKind::Code,
                group_branch: "yhtye/G-1".into(),
                base_branch: "main".into(),
            })
            .await;
        assert_eq!(
            ws,
            GitResult::Workspace {
                path: "/tmp/p".into()
            }
        );
        let merged = git
            .run(&GitOp::MergeGroup {
                group: "G-1".into(),
                worktree: "/tmp/p".into(),
                group_branch: "yhtye/G-1".into(),
                base_branch: "main".into(),
                trigger: crate::domain::MergeTrigger::FinishGroup,
            })
            .await;
        assert!(matches!(merged, GitResult::Merged { .. }));
    }
}
