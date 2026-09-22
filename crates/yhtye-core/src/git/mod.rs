//! Git operations behind a trait (`core-design.md` §7). Stage 3a ships only
//! [`NoopGit`] (everything happens in the project directory, merges always
//! succeed); the `git` CLI implementation arrives in Stage 3c.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::domain::{GitOp, GitResult};

/// Runs git for the orchestration. Every method is called from the runtime loop
/// and should finish quickly (plain git commands, no network).
#[async_trait]
pub trait GitService: Send + Sync + 'static {
    /// Branch checked out in the project's main worktree (`None`: detached HEAD).
    async fn current_branch(&self) -> Result<Option<String>, String>;

    /// Runs `op`. Expected outcomes (conflicts, dirty trees) are results, not errors:
    /// `CreateGroupBranch` / `RemoveWorkspace` → `Done`; `PrepareWorkspace` →
    /// `Workspace`; `FinishTask` → `Merged` / `Conflict` / `Dirty`;
    /// `MergeGroup` → `Merged` / `Blocked`; anything else going wrong → `Failed`.
    async fn run(&self, op: &GitOp) -> GitResult;
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

    async fn run(&self, op: &GitOp) -> GitResult {
        match op {
            GitOp::CreateGroupBranch { .. } | GitOp::RemoveWorkspace { .. } => GitResult::Done,
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
                group_branch: "yhtye/G-1".into(),
                base_branch: "main".into(),
            })
            .await;
        assert!(matches!(merged, GitResult::Merged { .. }));
    }
}
