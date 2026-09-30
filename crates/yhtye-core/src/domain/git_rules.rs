//! Results of git operations (`orchestration-model.md` §6).

use serde_json::json;

use super::command::{Effect, GitOp, GitResult, MergeTrigger};
use super::event::DomainEvent;
use super::inbox::{InboxItem, InboxKind};
use super::machine::Tx;
use super::state::{GroupStatus, TaskStatus};
use super::types::{HelpKind, ToolError};

impl Tx {
    pub(super) fn git_done(&mut self, op: GitOp, result: GitResult) {
        match op {
            GitOp::CreateGroupBranch { group, .. } => self.group_branch_created(&group, result),
            GitOp::PrepareWorkspace { task, .. } => self.workspace_prepared(&task, result),
            GitOp::FinishTask { task, .. } => self.task_finished(&task, result),
            GitOp::RemoveWorkspace { task, .. } => {
                if let GitResult::Failed { message } = result {
                    tracing::warn!("removing the worktree of {task} failed: {message}");
                }
            }
            GitOp::RemoveGroupWorkspace { group } => {
                if let GitResult::Failed { message } = result {
                    tracing::warn!(
                        "removing the integration worktree of {group} failed: {message}"
                    );
                }
            }
            GitOp::MergeGroup { group, trigger, .. } => self.group_merged(&group, result, trigger),
        }
    }

    fn group_branch_created(&mut self, group: &str, result: GitResult) {
        let GitResult::Failed { message } = result else {
            return;
        };
        self.emit(DomainEvent::GroupCancelled {
            group: group.to_string(),
            reason: format!("could not create the group branch: {message}"),
        });
        self.reply(Err(ToolError::internal(format!(
            "could not create the group branch for {group}: {message}"
        ))));
    }

    fn workspace_prepared(&mut self, task: &str, result: GitResult) {
        let Some(t) = self.state.task(task) else {
            return;
        };
        if t.status != TaskStatus::Running || t.workdir.is_some() {
            return;
        }
        match result {
            GitResult::Workspace { path } => {
                self.emit(DomainEvent::WorkspaceReady {
                    task: task.to_string(),
                    path,
                });
                self.begin_task(task, None);
            }
            other => {
                let message = format!(
                    "Could not prepare the task's worktree: {}",
                    describe(&other)
                );
                self.raise_yhtye_help(task, HelpKind::GitFailed, &message);
            }
        }
    }

    fn task_finished(&mut self, task: &str, result: GitResult) {
        let Some(t) = self.state.task(task) else {
            return;
        };
        if t.status != TaskStatus::Merging {
            return;
        }
        let index = t.current;
        let branches = t
            .branch()
            .map(|b| format!(" (`{b}` into `{}`)", super::state::group_branch(&t.group)))
            .unwrap_or_default();
        let failure = match result {
            GitResult::Done => Ok("finished".to_string()),
            GitResult::Merged { detail } => Ok(detail),
            other => Err(other),
        };
        let failure = match failure {
            Ok(detail) => {
                self.emit(DomainEvent::StepCompleted {
                    task: task.to_string(),
                    step: index,
                    result: detail,
                    verdict: None,
                });
                return self.complete_task(task);
            }
            Err(other) => other,
        };
        let (kind, message) = match failure {
            GitResult::Conflict { files } => (
                HelpKind::MergeConflict,
                format!(
                    "Merging the task branch into the group branch{branches} conflicted in: {}. The \
                     merge was aborted. Add an implement step (modify_steps) that merges the group \
                     branch into the task branch and resolves the conflict, then resume; the merge \
                     is retried at `done`.",
                    files.join(", ")
                ),
            ),
            GitResult::Dirty { files } => (
                HelpKind::DirtyReadonlyTree,
                format!(
                    "The read-only investigate task left changes in: {}. Yhtye moved them to a git \
                     stash (see `git stash list`) so the shared group worktree stays clean. Resume \
                     to finish the task (the changes stay in the stash), or cancel it.",
                    files.join(", ")
                ),
            ),
            other => (
                HelpKind::GitFailed,
                format!("Finishing the task in git failed: {}", describe(&other)),
            ),
        };
        // `done` runs again (the merge is retried) when the task resumes.
        self.emit(DomainEvent::StepReset {
            task: task.to_string(),
            step: index,
        });
        self.raise_yhtye_help(task, kind, &message);
    }

    /// `RetryGroupMerge`: a `merge_blocked` group goes back to `finishing` and
    /// the base merge runs again (the user fixed what blocked it).
    pub(super) fn retry_group_merge(&mut self, group: &str) -> Result<(), ToolError> {
        let g = self
            .state
            .group(group)
            .ok_or_else(|| ToolError::not_found(format!("no group {group}")))?;
        if g.status != GroupStatus::MergeBlocked {
            return Err(ToolError::invalid_state(format!(
                "group {} is {}; only a merge_blocked group can retry its merge",
                g.id,
                g.status.as_str()
            )));
        }
        if let Some(open) = self.state.open_group_on(&g.base_branch) {
            return Err(ToolError::conflict(format!(
                "group {} is {}; retry after it is done",
                open.id,
                open.status.as_str()
            )));
        }
        let op = GitOp::MergeGroup {
            group: g.id.clone(),
            group_branch: g.group_branch.clone(),
            base_branch: g.base_branch.clone(),
            trigger: MergeTrigger::UserRetry,
        };
        let summary = g.finish_summary.clone().unwrap_or_default();
        self.emit(DomainEvent::GroupFinishing {
            group: group.to_string(),
            summary,
        });
        self.effect(Effect::Git(op));
        Ok(())
    }

    fn group_merged(&mut self, group: &str, result: GitResult, trigger: MergeTrigger) {
        let (ok, detail) = match result {
            GitResult::Merged { detail } => (true, detail),
            GitResult::Done => (true, "merged".to_string()),
            other => (false, describe(&other)),
        };
        self.emit(DomainEvent::GroupMergeFinished {
            group: group.to_string(),
            ok,
            detail: detail.clone(),
        });
        let origin = match trigger {
            MergeTrigger::FinishGroup => None,
            MergeTrigger::UserRetry => Some("The user retried the merge from the Yhtye UI"),
            MergeTrigger::Yhtye => Some(
                "Yhtye finished the group because you ended your turn without finish_group \
                 after a reminder",
            ),
        };
        if let Some(origin) = origin {
            let ok_text = if ok { "true" } else { "false" };
            self.queue_inbox_group(
                group,
                InboxItem::new(
                    InboxKind::MergeResult,
                    &[("group", group), ("ok", ok_text)],
                    format!("{origin}: {detail}"),
                ),
            );
        }
        let status = self.state.group(group).map(|g| g.status);
        self.reply(Ok(json!({
            "group_id": group,
            "status": status,
            "merge": { "ok": ok, "detail": detail },
        })));
    }
}

fn describe(result: &GitResult) -> String {
    match result {
        GitResult::Done => "done".into(),
        GitResult::Workspace { path } => format!("workspace {}", path.display()),
        GitResult::Merged { detail } | GitResult::Blocked { detail } => detail.clone(),
        GitResult::Conflict { files } => format!("conflict in {}", files.join(", ")),
        GitResult::Dirty { files } => format!("uncommitted changes in {}", files.join(", ")),
        GitResult::Failed { message } => message.clone(),
    }
}
