//! [`GitCli`]: the [`GitService`] that runs the `git` CLI
//! (`orchestration-model.md` §6, `core-design.md` §7).
//!
//! Layout: the group branch is checked out in the group's integration worktree
//! `<root>/<groupId>/_group` (task branches are merged there, `investigate` tasks
//! read there); each `code` task has `<root>/<groupId>/<taskId>`. `<root>` is
//! outside the repository ([`worktree_root`]), so the user's tree stays clean.
//! Every operation accepts the results of an earlier, interrupted run of itself.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::branches;
use super::repo::{self, Hooks, MergeOutcome};
use super::run::git_ok;
use super::worktree::{ensure_worktree, is_worktree, remove_worktree};
use super::{BranchWorktreeError, CreateBranchError, GitService};
use crate::domain::{GitOp, GitResult, TaskKind, task_branch};

/// Most of the `git diff` given to a restarted agent (bytes).
const MAX_CHANGES: usize = 16 * 1024;

/// Directory holding the worktrees of `project` under Yhtye's data directory.
#[must_use]
pub fn worktree_root(data_dir: &Path, project: &str) -> PathBuf {
    data_dir.join("worktrees").join(project)
}

/// Git for one project: the repository's main worktree and where Yhtye keeps
/// its own worktrees.
#[derive(Debug, Clone)]
pub struct GitCli {
    repo: PathBuf,
    root: PathBuf,
}

/// The branches an operation works with.
struct Branches<'a> {
    group: &'a str,
    group_branch: &'a str,
    base_branch: &'a str,
}

type Step<T> = Result<T, String>;

/// `3` of `G-3` and of `G-3-T-1` (a group branch, a task branch, a directory).
fn group_number(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("G-")?;
    let digits = rest.split(|c: char| !c.is_ascii_digit()).next()?;
    digits.parse().ok()
}

impl GitCli {
    /// `repo`: the project's main worktree. `root`: where worktrees go (see
    /// [`worktree_root`]); it must be outside the repository.
    #[must_use]
    pub fn new(repo: impl Into<PathBuf>, root: impl Into<PathBuf>) -> Self {
        Self {
            repo: repo.into(),
            root: root.into(),
        }
    }

    /// Integration worktree of `group` (checked out on the group branch).
    #[must_use]
    pub fn group_dir(&self, group: &str) -> PathBuf {
        self.root.join(group).join("_group")
    }

    /// Worktree of `code` task `task`.
    #[must_use]
    pub fn task_dir(&self, group: &str, task: &str) -> PathBuf {
        self.root.join(group).join(task)
    }

    /// Creates the group branch from the base branch and its integration
    /// worktree. An existing branch is only accepted if it is still at the base
    /// branch's commit (Yhtye does not adopt unknown branches).
    async fn create_group(&self, b: &Branches<'_>) -> Step<GitResult> {
        if repo::branch_exists(&self.repo, b.group_branch).await? {
            let here = repo::rev(&self.repo, b.group_branch).await?;
            let base = repo::rev(&self.repo, b.base_branch).await?;
            if here != base {
                return Err(format!(
                    "branch {} already exists and is not at {}; delete or rename it",
                    b.group_branch, b.base_branch
                ));
            }
        }
        self.ensure_group(b).await?;
        Ok(GitResult::Done)
    }

    /// The group branch (recreated from the base branch if it is missing, e.g.
    /// Yhtye stopped while creating it) and its integration worktree.
    async fn ensure_group(&self, b: &Branches<'_>) -> Step<PathBuf> {
        if !repo::branch_exists(&self.repo, b.group_branch).await? {
            let start = format!("refs/heads/{}", b.base_branch);
            repo::rev(&self.repo, &start).await?;
            let args = ["branch", "--no-track", b.group_branch, &start];
            super::run::git_ok(&self.repo, &args).await?;
        }
        let dir = self.group_dir(b.group);
        ensure_worktree(&self.repo, &dir, b.group_branch).await?;
        Ok(dir)
    }

    async fn prepare(&self, b: &Branches<'_>, task: &str, kind: TaskKind) -> Step<GitResult> {
        let group_dir = self.ensure_group(b).await?;
        if kind == TaskKind::Investigate {
            return Ok(GitResult::Workspace { path: group_dir });
        }
        let branch = task_branch(b.group, task);
        if !repo::branch_exists(&self.repo, &branch).await? {
            let args = ["branch", "--no-track", branch.as_str(), b.group_branch];
            super::run::git_ok(&self.repo, &args).await?;
        }
        let dir = self.task_dir(b.group, task);
        ensure_worktree(&self.repo, &dir, &branch).await?;
        Ok(GitResult::Workspace { path: dir })
    }

    /// `code`: commits what the agents left, merges the task branch into the
    /// group branch (in the integration worktree) and removes the task worktree.
    async fn finish_code(&self, b: &Branches<'_>, task: &str, message: &str) -> Step<GitResult> {
        let dir = self.task_dir(b.group, task);
        let branch = task_branch(b.group, task);
        if is_worktree(&self.repo, &dir).await? {
            refuse_unfinished(&dir, &["a merge"]).await?;
            let unresolved = repo::unresolved_files(&dir).await?;
            if !unresolved.is_empty() {
                return Ok(GitResult::Conflict { files: unresolved });
            }
            repo::commit_all(&dir, message).await?;
        }
        if !repo::branch_exists(&self.repo, &branch).await? {
            return Err(format!("the task branch {branch} does not exist"));
        }
        let group_dir = self.ensure_group(b).await?;
        let result = if repo::is_ancestor(&group_dir, &branch, "HEAD").await? {
            // Nothing to merge: no changes, or merged before a restart.
            GitResult::Merged {
                detail: format!("{branch} is already in {}", b.group_branch),
            }
        } else {
            self.clean_integration(&group_dir).await?;
            let msg = format!("Merge {branch} ({task}) into {}", b.group_branch);
            match repo::merge_no_ff(&group_dir, &branch, &msg, Hooks::Skip).await? {
                MergeOutcome::Merged(head) => GitResult::Merged {
                    detail: format!("merged {branch} into {} ({})", b.group_branch, short(&head)),
                },
                MergeOutcome::Conflict(files) => return Ok(GitResult::Conflict { files }),
                MergeOutcome::Failed(message) => return Err(message),
            }
        };
        if let Err(e) = remove_worktree(&self.repo, &dir).await {
            tracing::warn!("removing the worktree of {task} failed: {e}");
        }
        Ok(result)
    }

    /// Makes sure the integration worktree can take a merge: a merge left
    /// behind by an interrupted run is aborted (the tree is Yhtye's own).
    async fn clean_integration(&self, dir: &Path) -> Step<()> {
        if let Some(op) = repo::operation_in_progress(dir).await? {
            if op != "a merge" {
                return Err(format!("{op} is in progress in {}", dir.display()));
            }
            super::run::git_ok(dir, &["merge", "--abort"]).await?;
        }
        let dirty = repo::tracked_changes(dir).await?;
        if dirty.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "the group's integration worktree {} has uncommitted changes in: {}",
                dir.display(),
                dirty.join(", ")
            ))
        }
    }

    /// `investigate`: the shared tree must still be clean. Changes left behind
    /// are moved to a stash (kept, not lost) and reported as `Dirty`.
    async fn finish_investigate(&self, b: &Branches<'_>, task: &str) -> Step<GitResult> {
        let dir = self.ensure_group(b).await?;
        refuse_unfinished(&dir, &[]).await?;
        let files = repo::changed_files(&dir).await?;
        if files.is_empty() {
            return Ok(GitResult::Done);
        }
        let msg = format!("yhtye: changes left by read-only task {task} ({})", b.group);
        repo::stash_all(&dir, &msg).await?;
        Ok(GitResult::Dirty { files })
    }

    /// A cancelled task: its uncommitted work is committed to its branch (which
    /// is kept), then the worktree is removed.
    async fn remove_task(&self, group: &str, task: &str) -> Step<GitResult> {
        let dir = self.task_dir(group, task);
        if is_worktree(&self.repo, &dir).await? {
            let msg = format!("{task}: work in progress (task cancelled; saved by Yhtye)");
            repo::commit_all(&dir, &msg).await?;
        }
        remove_worktree(&self.repo, &dir).await?;
        Ok(GitResult::Done)
    }

    /// A cancelled group: an interrupted merge in its integration worktree is
    /// aborted, what is left there is committed to the group branch (which is
    /// kept, like task branches), then the worktree is removed.
    async fn remove_group(&self, group: &str) -> Step<GitResult> {
        let dir = self.group_dir(group);
        if is_worktree(&self.repo, &dir).await? {
            if repo::operation_in_progress(&dir).await? == Some("a merge") {
                super::run::git_ok(&dir, &["merge", "--abort"]).await?;
            }
            refuse_unfinished(&dir, &[]).await?;
            let msg = format!("{group}: work in progress (group cancelled; saved by Yhtye)");
            repo::commit_all(&dir, &msg).await?;
        }
        remove_worktree(&self.repo, &dir).await?;
        Ok(GitResult::Done)
    }

    /// Merges the group branch into the base branch in the base branch's
    /// worktree (resolved now: the main worktree if it has the branch checked
    /// out, any other worktree of it, or one Yhtye creates), if it has no
    /// uncommitted changes to tracked files and no operation in progress.
    /// Anything in the way is `Blocked`; the user's tree is never changed
    /// except by a successful merge.
    async fn merge_group(&self, b: &Branches<'_>) -> Step<GitResult> {
        let dir = match self.resolve(b.base_branch).await {
            Ok(dir) => dir,
            Err(e) => {
                return Ok(GitResult::Blocked {
                    detail: e.to_string(),
                });
            }
        };
        if let Some(reason) = base_blocker(&dir, b.base_branch).await? {
            return Ok(GitResult::Blocked { detail: reason });
        }
        let result = if repo::is_ancestor(&dir, b.group_branch, "HEAD").await? {
            GitResult::Merged {
                detail: format!("{} is already in {}", b.group_branch, b.base_branch),
            }
        } else {
            let msg = format!(
                "Merge {} ({}) into {}",
                b.group_branch, b.group, b.base_branch
            );
            match repo::merge_no_ff(&dir, b.group_branch, &msg, Hooks::Run).await? {
                MergeOutcome::Merged(head) => GitResult::Merged {
                    detail: format!(
                        "merged {} into {} ({})",
                        b.group_branch,
                        b.base_branch,
                        short(&head)
                    ),
                },
                MergeOutcome::Conflict(files) => GitResult::Blocked {
                    detail: format!(
                        "merging {} into {} conflicts in: {} (the merge was aborted)",
                        b.group_branch,
                        b.base_branch,
                        files.join(", ")
                    ),
                },
                MergeOutcome::Failed(message) => GitResult::Blocked { detail: message },
            }
        };
        if matches!(result, GitResult::Merged { .. })
            && let Err(e) = remove_worktree(&self.repo, &self.group_dir(b.group)).await
        {
            tracing::warn!(
                "removing the integration worktree of {} failed: {e}",
                b.group
            );
        }
        Ok(result)
    }

    async fn resolve(&self, branch: &str) -> Result<PathBuf, BranchWorktreeError> {
        branches::resolve(&self.repo, &self.root, branch).await
    }

    async fn run_op(&self, op: &GitOp) -> Step<GitResult> {
        match op {
            GitOp::CreateGroupBranch {
                group,
                group_branch,
                base_branch,
            } => {
                let b = branch_set(group, group_branch, base_branch);
                self.create_group(&b).await
            }
            GitOp::PrepareWorkspace {
                group,
                task,
                kind,
                group_branch,
                base_branch,
            } => {
                let b = branch_set(group, group_branch, base_branch);
                self.prepare(&b, task, *kind).await
            }
            GitOp::FinishTask {
                group,
                task,
                kind,
                message,
                group_branch,
                base_branch,
            } => {
                let b = branch_set(group, group_branch, base_branch);
                match kind {
                    TaskKind::Code => self.finish_code(&b, task, message).await,
                    TaskKind::Investigate => self.finish_investigate(&b, task).await,
                }
            }
            GitOp::RemoveWorkspace { group, task } => self.remove_task(group, task).await,
            GitOp::RemoveGroupWorkspace { group } => self.remove_group(group).await,
            GitOp::MergeGroup {
                group,
                group_branch,
                base_branch,
                ..
            } => {
                let b = branch_set(group, group_branch, base_branch);
                self.merge_group(&b).await
            }
        }
    }
}

/// Why the worktree `dir` of `base` cannot take the group merge now, if anything.
async fn base_blocker(dir: &Path, base: &str) -> Step<Option<String>> {
    if let Some(op) = repo::operation_in_progress(dir).await? {
        return Ok(Some(format!(
            "{op} is in progress in {}; finish or abort it and retry",
            dir.display()
        )));
    }
    let dirty = repo::tracked_changes(dir).await?;
    if !dirty.is_empty() {
        return Ok(Some(format!(
            "the worktree of {base} ({}) has uncommitted changes in: {}; commit or stash them \
             and retry",
            dir.display(),
            dirty.join(", ")
        )));
    }
    Ok(None)
}

fn branch_set<'a>(group: &'a str, group_branch: &'a str, base_branch: &'a str) -> Branches<'a> {
    Branches {
        group,
        group_branch,
        base_branch,
    }
}

/// Fails if a rebase / cherry-pick / revert (or anything not in `allowed`) is
/// in progress in `dir`: committing or merging past it would lose that work.
async fn refuse_unfinished(dir: &Path, allowed: &[&str]) -> Step<()> {
    match repo::operation_in_progress(dir).await? {
        Some(op) if !allowed.contains(&op) => Err(format!(
            "{op} is in progress in {}; finish or abort it, then resume",
            dir.display()
        )),
        _ => Ok(()),
    }
}

fn short(commit: &str) -> &str {
    commit.get(..10).unwrap_or(commit)
}

#[async_trait]
impl GitService for GitCli {
    async fn current_branch(&self) -> Result<Option<String>, String> {
        repo::current_branch(&self.repo).await
    }

    async fn branch_exists(&self, branch: &str) -> Result<bool, String> {
        repo::branch_exists(&self.repo, branch).await
    }

    async fn worktree_branch(&self, dir: &Path) -> Result<Option<String>, String> {
        if !dir.is_dir() {
            return Ok(None);
        }
        repo::current_branch(dir).await
    }

    async fn was_renamed(&self, from: &str, to: &str) -> Result<bool, String> {
        repo::was_renamed(&self.repo, from, to).await
    }

    async fn resolve_branch_worktree(&self, branch: &str) -> Result<PathBuf, BranchWorktreeError> {
        self.resolve(branch).await
    }

    async fn create_branch(
        &self,
        name: &str,
        from: Option<&str>,
    ) -> Result<PathBuf, CreateBranchError> {
        branches::create(&self.repo, &self.root, name, from).await
    }

    async fn discard_branch(&self, name: &str) -> Result<(), String> {
        branches::discard(&self.repo, name).await
    }

    async fn highest_group_number(&self) -> Result<u32, String> {
        let refs = git_ok(
            &self.repo,
            &["for-each-ref", "--format=%(refname)", "refs/heads/yhtye/"],
        )
        .await?;
        let mut highest = refs
            .lines()
            .filter_map(|r| r.strip_prefix("refs/heads/yhtye/"))
            .filter_map(group_number)
            .max()
            .unwrap_or(0);
        // A missing worktree root just means no group had a worktree.
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            let dirs = entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok());
            highest = dirs
                .filter_map(|d| group_number(&d))
                .fold(highest, u32::max);
        }
        Ok(highest)
    }

    async fn run(&self, op: &GitOp) -> GitResult {
        match self.run_op(op).await {
            Ok(result) => result,
            Err(message) => {
                tracing::warn!("git {op:?} failed: {message}");
                GitResult::Failed { message }
            }
        }
    }

    async fn workspace_changes(
        &self,
        workdir: &Path,
        group_branch: &str,
    ) -> Result<String, String> {
        let base = match repo::git_merge_base(workdir, group_branch).await {
            Ok(base) => base,
            Err(_) => group_branch.to_string(),
        };
        repo::changes_since(workdir, &base, MAX_CHANGES).await
    }
}
