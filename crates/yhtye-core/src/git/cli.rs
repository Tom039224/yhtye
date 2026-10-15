//! [`GitCli`]: the [`GitService`] that runs the `git` CLI
//! (`orchestration-model.md` §6, `core-design.md` §7).
//!
//! Layout: the group branch is checked out in the group's integration worktree
//! `<root>/<group>/_group` (task branches are squashed into it, `investigate`
//! tasks read there); each `code` task has `<root>/<group>/<task>` (ids: names, or
//! the numbered `G-<n>` / `T-<n>` of older projects). `<root>` is
//! outside the repository ([`worktree_root`]), so the user's tree stays clean.
//! Every operation accepts the results of an earlier, interrupted run of itself.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use super::branches;
use super::message::{self, GroupMessage, SquashMessage};
use super::repo::{self, MergeOutcome, SquashOutcome};
use super::run::git_ok;
use super::worktree::{ensure_worktree, is_worktree, listed_path, remove_worktree};
use super::{BranchWorktreeError, CreateBranchError, GitService};
use crate::agents::AgentChoice;
use crate::domain::{GitOp, GitResult, GroupTask, TaskKind, task_branch};

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
    before_group_merge: Option<TestHook>,
}

/// Test hook run in the chat's worktree between checking it and merging a
/// group there (to simulate someone switching branches at that moment).
#[derive(Clone)]
struct TestHook(Arc<dyn Fn(&Path) + Send + Sync>);

impl std::fmt::Debug for TestHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TestHook")
    }
}

/// The branches an operation works with.
struct Branches<'a> {
    group: &'a str,
    group_branch: &'a str,
    base_branch: &'a str,
}

/// What the squash commit of a finished task says besides its changes.
struct TaskInfo<'a> {
    task: &'a str,
    /// Message of the commit of what the agents left uncommitted.
    message: &'a str,
    title: &'a str,
    agent: Option<&'a AgentChoice>,
    reviewers: &'a [AgentChoice],
}

/// What the merge commit of a group says besides its changes.
struct GroupInfo<'a> {
    title: &'a str,
    summary: &'a str,
    tasks: &'a [GroupTask],
}

type Step<T> = Result<T, String>;

/// The branch (of `yhtye_branches`: the names after `yhtye/`) that already uses
/// the name of group `group` (`task: None`) or of task `task`, if any: the group
/// branch or one of the group's task branches; any task branch of that task
/// name. Compared ignoring case, since refs may live on a case-insensitive
/// file system.
fn branch_using_name<'a>(
    yhtye_branches: &'a [String],
    group: &str,
    task: Option<&str>,
) -> Option<&'a str> {
    let group = group.to_ascii_lowercase();
    let uses = |branch: &str| match task {
        None => {
            branch == group
                || branch
                    .strip_prefix(group.as_str())
                    .is_some_and(|rest| rest.starts_with("--") || rest.starts_with('/'))
        }
        Some(task) => branch
            .split_once("--")
            .is_some_and(|(_, t)| t.eq_ignore_ascii_case(task)),
    };
    yhtye_branches
        .iter()
        .find(|b| uses(&b.to_ascii_lowercase()))
        .map(String::as_str)
}

/// The entry of directory `dir` named `name` (ignoring case), if any.
fn entry_named(dir: &Path, name: &str) -> Option<PathBuf> {
    // A missing directory just means no worktree was made there.
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

impl GitCli {
    /// `repo`: the project's main worktree. `root`: where worktrees go (see
    /// [`worktree_root`]); it must be outside the repository.
    #[must_use]
    pub fn new(repo: impl Into<PathBuf>, root: impl Into<PathBuf>) -> Self {
        Self {
            repo: repo.into(),
            root: root.into(),
            before_group_merge: None,
        }
    }

    /// For tests only: runs `hook` (with the chat's worktree) after a group
    /// merge's checks and right before git merges.
    #[doc(hidden)]
    #[must_use]
    pub fn before_group_merge(mut self, hook: impl Fn(&Path) + Send + Sync + 'static) -> Self {
        self.before_group_merge = Some(TestHook(Arc::new(hook)));
        self
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

    /// `code`: commits what the agents left, squashes the task branch into the
    /// group branch as one commit (in the integration worktree) and removes the
    /// task worktree.
    async fn finish_code(&self, b: &Branches<'_>, info: &TaskInfo<'_>) -> Step<GitResult> {
        let task = info.task;
        let dir = self.task_dir(b.group, task);
        let branch = task_branch(b.group, task);
        if is_worktree(&self.repo, &dir).await? {
            refuse_unfinished(&dir, &["a merge"]).await?;
            let unresolved = repo::unresolved_files(&dir).await?;
            if !unresolved.is_empty() {
                return Ok(GitResult::Conflict { files: unresolved });
            }
            repo::commit_all(&dir, info.message).await?;
        }
        if !repo::branch_exists(&self.repo, &branch).await? {
            return Err(format!("the task branch {branch} does not exist"));
        }
        let group_dir = self.ensure_group(b).await?;
        let result = if already_in_group(&group_dir, &branch, task).await? {
            // Nothing to merge: no changes, or merged before a restart.
            GitResult::Merged {
                detail: format!("{branch} is already in {}", b.group_branch),
            }
        } else {
            self.clean_integration(&group_dir).await?;
            match squash_task(&group_dir, b, &branch, info).await? {
                Squashed::Done(result) => result,
                Squashed::Conflict(files) => return Ok(GitResult::Conflict { files }),
            }
        };
        if let Err(e) = remove_worktree(&self.repo, &dir).await {
            tracing::warn!("removing the worktree of {task} failed: {e}");
        }
        Ok(result)
    }

    /// Makes sure the integration worktree can take a merge: a merge or squash
    /// left behind by an interrupted run is aborted (the tree is Yhtye's own).
    async fn clean_integration(&self, dir: &Path) -> Step<()> {
        repo::abort_squash(dir).await?;
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
        // Not the leftovers of a task merge, which is not this task's doing.
        repo::abort_squash(&dir).await?;
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
            repo::abort_squash(&dir).await?;
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

    /// Merges the group branch into the base branch in the chat's worktree
    /// `dir`, checked right before (Stage 8e): the worktree must exist and have
    /// the base branch checked out (else nothing is merged and the detail tells
    /// the orchestrator what it found), have no uncommitted changes to tracked
    /// files and no operation in progress. Anything in the way is `Blocked`; the
    /// user's tree is never changed except by a successful merge. The check and
    /// the merge are not atomic (another chat or the user may switch branches in
    /// between), so where the merge landed is checked afterwards.
    async fn merge_group(
        &self,
        dir: &Path,
        b: &Branches<'_>,
        info: &GroupInfo<'_>,
    ) -> Step<GitResult> {
        if let Some(reason) = base_mismatch(dir, b).await? {
            return Ok(GitResult::Blocked { detail: reason });
        }
        if let Some(reason) = base_blocker(dir, b.base_branch).await? {
            return Ok(GitResult::Blocked { detail: reason });
        }
        let dir = dir.to_path_buf();
        let before = repo::rev(&dir, &format!("refs/heads/{}", b.base_branch)).await?;
        if let Some(TestHook(hook)) = &self.before_group_merge {
            hook(&dir);
        }
        // The base branch itself, not `HEAD` (which may have been switched).
        let result = if repo::is_ancestor(&dir, b.group_branch, &before).await? {
            GitResult::Merged {
                detail: format!("{} is already in {}", b.group_branch, b.base_branch),
            }
        } else {
            let msg = group_commit_message(&dir, b, &before, info).await?;
            match repo::merge_no_ff(&dir, b.group_branch, &msg).await? {
                MergeOutcome::Merged(head) => landing(&dir, b, &before, &head).await?,
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
                title,
                agent,
                reviewers,
            } => {
                let b = branch_set(group, group_branch, base_branch);
                match kind {
                    TaskKind::Code => {
                        let info = TaskInfo {
                            task,
                            message,
                            title,
                            agent: agent.as_ref(),
                            reviewers,
                        };
                        self.finish_code(&b, &info).await
                    }
                    TaskKind::Investigate => self.finish_investigate(&b, task).await,
                }
            }
            GitOp::RemoveWorkspace { group, task } => self.remove_task(group, task).await,
            GitOp::RemoveGroupWorkspace { group } => self.remove_group(group).await,
            GitOp::MergeGroup {
                group,
                worktree,
                group_branch,
                base_branch,
                title,
                summary,
                tasks,
                ..
            } => {
                let b = branch_set(group, group_branch, base_branch);
                let info = GroupInfo {
                    title,
                    summary,
                    tasks,
                };
                self.merge_group(worktree, &b, &info).await
            }
        }
    }
}

/// Whether the work of the task branch `branch` is already in the group
/// branch checked out in `group_dir`: the branch is in its history (no changes,
/// or merged with `--no-ff` by an older Yhtye), or a squash commit that names
/// the task (`Yhtye-Task:`) was made after the branch forked from it.
async fn already_in_group(group_dir: &Path, branch: &str, task: &str) -> Step<bool> {
    if repo::is_ancestor(group_dir, branch, "HEAD").await? {
        return Ok(true);
    }
    let squashed = repo::merged_tasks(group_dir, &format!("{branch}..HEAD")).await?;
    Ok(squashed.iter().any(|t| t == task))
}

/// Result of squashing a task branch.
enum Squashed {
    Done(GitResult),
    Conflict(Vec<String>),
}

/// Squashes `branch` into the group branch checked out in `group_dir`, which
/// is clean: one commit with the task's title, its commits' subjects and the
/// trailers. A conflict leaves `group_dir` clean again.
async fn squash_task(
    group_dir: &Path,
    b: &Branches<'_>,
    branch: &str,
    info: &TaskInfo<'_>,
) -> Step<Squashed> {
    match repo::merge_squash(group_dir, branch).await? {
        SquashOutcome::Conflict(files) => Ok(Squashed::Conflict(files)),
        SquashOutcome::Failed(message) => Err(message),
        SquashOutcome::Nothing => Ok(Squashed::Done(GitResult::Merged {
            detail: format!("{branch} has no changes for {}", b.group_branch),
        })),
        SquashOutcome::Staged => {
            let range = format!("{}..{branch}", b.group_branch);
            let commits = repo::commit_subjects(group_dir, &range).await?;
            let msg = message::squash_message(&SquashMessage {
                group: b.group,
                task: info.task,
                title: info.title,
                commits: &commits,
                implementer: info.agent,
                reviewers: info.reviewers,
            });
            match repo::commit_squash(group_dir, &msg).await {
                Ok(head) => Ok(Squashed::Done(GitResult::Merged {
                    detail: format!(
                        "merged {branch} into {} as one commit ({})",
                        b.group_branch,
                        short(&head)
                    ),
                })),
                Err(e) => {
                    // Leave the tree clean for the retry.
                    if let Err(abort) = repo::abort_squash(group_dir).await {
                        tracing::warn!("undoing the squash of {branch} failed: {abort}");
                    }
                    Err(e)
                }
            }
        }
    }
}

/// The message of the merge of group `b.group` into the base branch, whose
/// tip is `base_tip`: the tasks are listed in the order they were merged into
/// the group.
async fn group_commit_message(
    dir: &Path,
    b: &Branches<'_>,
    base_tip: &str,
    info: &GroupInfo<'_>,
) -> Step<String> {
    let range = format!("{base_tip}..{}", b.group_branch);
    let merged = repo::merged_tasks(dir, &range).await?;
    let tasks = message::in_merge_order(info.tasks, &merged);
    Ok(message::group_message(&GroupMessage {
        group: b.group,
        group_branch: b.group_branch,
        base_branch: b.base_branch,
        title: info.title,
        summary: info.summary,
        tasks: &tasks,
    }))
}

/// Why the group cannot be merged in `dir` because it is not on the group's
/// base branch any more (renamed, switched, detached, or gone), if so. The
/// text is for the orchestrator: what it found and how to go on.
async fn base_mismatch(dir: &Path, b: &Branches<'_>) -> Step<Option<String>> {
    let base = b.base_branch;
    let shown = dir.display();
    let now = if !dir.is_dir() {
        format!("the working tree {shown} no longer exists")
    } else {
        match repo::current_branch(dir).await? {
            Some(branch) if branch == base => return Ok(None),
            Some(branch) => {
                format!("the working tree {shown} now has branch `{branch}` checked out")
            }
            None => format!("the working tree {shown} is now at a detached HEAD"),
        }
    };
    Ok(Some(format!(
        "Nothing was merged: group {} was to be merged into `{base}`, but {now}. Put the \
         working tree back on `{base}` and call finish_group again, or call finish_group with \
         `into` set to the branch checked out now to merge there instead.",
        b.group
    )))
}

/// Whether the merge git just made in `dir` (now at `head`) landed on the base
/// branch on top of `before` (its commit when the checks passed). If not (the
/// worktree was switched, or the base branch moved, while merging) it is
/// `Blocked` with where it went; nothing is undone, the user decides.
async fn landing(dir: &Path, b: &Branches<'_>, before: &str, head: &str) -> Step<GitResult> {
    let base = b.base_branch;
    let now = repo::current_branch(dir).await?;
    let parent = repo::rev_opt(dir, "HEAD^1").await?;
    if now.as_deref() == Some(base) && parent.as_deref() == Some(before) {
        return Ok(GitResult::Merged {
            detail: format!("merged {} into {base} ({})", b.group_branch, short(head)),
        });
    }
    let shown = dir.display();
    let went = match now {
        Some(branch) if branch != base => {
            format!("branch `{branch}` (checked out in {shown} when git merged), not to `{base}`")
        }
        Some(_) => format!(
            "`{base}`, but `{base}` had moved from {} while merging",
            short(before)
        ),
        None => format!("a detached HEAD in {shown}, not to `{base}`"),
    };
    Ok(GitResult::Blocked {
        detail: format!(
            "The working tree changed while group {} was being merged: the merge of {} (now \
             {}) went to {went}. Yhtye undid nothing. Check that branch (revert the merge there \
             if it was not wanted), put {shown} back on `{base}` and call finish_group again.",
            b.group,
            b.group_branch,
            short(head)
        ),
    })
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

    async fn find_worktree(&self, dir: &Path) -> Result<Option<PathBuf>, String> {
        listed_path(&self.repo, dir).await
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

    async fn name_in_git(&self, group: &str, task: Option<&str>) -> Result<Option<String>, String> {
        let refs = git_ok(
            &self.repo,
            &["for-each-ref", "--format=%(refname)", "refs/heads/yhtye/"],
        )
        .await?;
        let branches: Vec<String> = refs
            .lines()
            .filter_map(|r| r.strip_prefix("refs/heads/yhtye/"))
            .map(str::to_string)
            .collect();
        if let Some(branch) = branch_using_name(&branches, group, task) {
            return Ok(Some(format!("git already has the branch yhtye/{branch}")));
        }
        let dir = match task {
            None => entry_named(&self.root, group),
            Some(task) => entry_named(&self.root.join(group), task),
        };
        Ok(dir.map(|d| format!("the worktree directory {} already exists", d.display())))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn branches(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

    #[test]
    fn a_group_name_is_used_by_its_branch_or_its_task_branches() {
        let b = branches(&["G-1", "G-1-T-2", "api", "web--login", "old/x"]);
        assert_eq!(branch_using_name(&b, "api", None), Some("api"));
        assert_eq!(branch_using_name(&b, "web", None), Some("web--login"));
        assert_eq!(branch_using_name(&b, "old", None), Some("old/x"));
        assert_eq!(
            branch_using_name(&b, "g-1", None),
            Some("G-1"),
            "ignoring case"
        );
        for free in ["ap", "api-v2", "we", "login", "g-1-t-2x"] {
            assert_eq!(branch_using_name(&b, free, None), None, "{free}");
        }
    }

    #[test]
    fn a_task_name_is_used_by_a_task_branch_of_any_group() {
        let b = branches(&["G-1", "G-1-T-2", "api", "web--login"]);
        assert_eq!(
            branch_using_name(&b, "api", Some("login")),
            Some("web--login")
        );
        for free in ["web", "api", "T-2", "t-2", "log"] {
            assert_eq!(branch_using_name(&b, "api", Some(free)), None, "{free}");
        }
    }
}
