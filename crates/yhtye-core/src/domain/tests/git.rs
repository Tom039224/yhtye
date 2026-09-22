//! Git outcomes: merge conflicts, dirty investigate trees, failures, blocked
//! group merges (`orchestration-model.md` §6).

use std::cell::Cell;
use std::rc::Rc;

use serde_json::json;

use super::*;

/// A git whose `FinishTask` returns `first` once, then merges.
fn finish_once(first: GitResult) -> GitResponder {
    let used = Rc::new(Cell::new(false));
    Box::new(move |op| match op {
        GitOp::FinishTask { .. } if !used.replace(true) => first.clone(),
        other => noop_git(other),
    })
}

#[test]
fn merge_conflict_raises_help_and_retries_after_a_fix_step() {
    let mut sim = Sim::new();
    sim.git = finish_once(GitResult::Conflict {
        files: vec!["README.md".into()],
    });
    sim.group();
    sim.task(json!({}));
    sim.report("T-1", Role::Implementer, None);
    assert_eq!(sim.status("T-1"), TaskStatus::Handling);
    let help = sim.state.open_help("T-1").expect("help").clone();
    assert_eq!(help.kind, HelpKind::MergeConflict);
    assert!(help.message.contains("README.md"));
    assert_eq!(
        sim.t("T-1").steps[1].status,
        StepStatus::Pending,
        "done reset"
    );

    let (reply, _) = sim.orch_ok(
        ToolName::ModifySteps,
        json!({"task_id": "T-1", "steps": [{"kind": "implement", "instruction": "merge the group branch and resolve"}]}),
    );
    assert_eq!(reply["steps"].as_array().map(Vec::len), Some(3));
    let (_, chain) = sim.orch_ok(
        ToolName::AnswerHelp,
        json!({"help_id": help.id, "action": "resume"}),
    );
    assert_eq!(
        run_steps(&chain),
        vec![("T-1".into(), Role::Implementer, 1)]
    );
    assert!(prompt_of(&chain).contains("merge the group branch and resolve"));
    sim.report("T-1", Role::Implementer, None);
    assert_eq!(sim.status("T-1"), TaskStatus::Done);
}

#[test]
fn resume_without_changes_retries_the_merge() {
    let mut sim = Sim::new();
    sim.git = finish_once(GitResult::Conflict { files: vec![] });
    sim.group();
    sim.task(json!({}));
    sim.report("T-1", Role::Implementer, None);
    sim.orch_ok(
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "resume"}),
    );
    assert_eq!(sim.status("T-1"), TaskStatus::Done);
}

#[test]
fn dirty_investigate_tree_raises_help() {
    let mut sim = Sim::new();
    sim.git = finish_once(GitResult::Dirty {
        files: vec!["x.txt".into()],
    });
    sim.group();
    sim.task(json!({"kind": "investigate"}));
    sim.report("T-1", Role::Implementer, None);
    let help = sim.state.open_help("T-1").expect("help");
    assert_eq!(help.kind, HelpKind::DirtyReadonlyTree);
}

#[test]
fn workspace_failure_raises_git_failed_and_resume_retries() {
    let mut sim = Sim::new();
    let failed = Rc::new(Cell::new(false));
    sim.git = Box::new(move |op| match op {
        GitOp::PrepareWorkspace { .. } if !failed.replace(true) => GitResult::Failed {
            message: "worktree add failed".into(),
        },
        other => noop_git(other),
    });
    sim.group();
    let chain = sim.task(json!({}));
    assert!(run_steps(&chain).is_empty());
    assert_eq!(
        sim.state.open_help("T-1").map(|h| h.kind),
        Some(HelpKind::GitFailed)
    );
    let (_, chain) = sim.orch_ok(
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "resume"}),
    );
    assert_eq!(
        run_steps(&chain),
        vec![("T-1".into(), Role::Implementer, 0)]
    );
}

#[test]
fn group_branch_failure_cancels_the_group_and_fails_the_call() {
    let mut sim = Sim::new();
    sim.git = Box::new(|op| match op {
        GitOp::CreateGroupBranch { .. } => GitResult::Failed {
            message: "branch exists".into(),
        },
        other => noop_git(other),
    });
    let chain = sim
        .tool(orch(), ToolName::CreateGroup, json!({"title": "g"}))
        .expect("the command itself succeeds");
    let err = chain.reply.expect("reply").expect_err("error reply");
    assert_eq!(err.code, ErrorCode::Internal);
    assert_eq!(
        sim.state.group("G-1").map(|g| g.status),
        Some(GroupStatus::Cancelled)
    );
}

#[test]
fn blocked_group_merge_is_merge_blocked_and_can_be_cancelled() {
    let mut sim = Sim::new();
    sim.git = Box::new(|op| match op {
        GitOp::MergeGroup { .. } => GitResult::Blocked {
            detail: "main worktree is dirty".into(),
        },
        other => noop_git(other),
    });
    sim.group();
    let (reply, _) = sim.orch_ok(
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "nothing"}),
    );
    assert_eq!(reply["status"], json!("merge_blocked"));
    assert_eq!(
        reply["merge"],
        json!({"ok": false, "detail": "main worktree is dirty"})
    );
    // A merge_blocked group does not block a new one, and can be cancelled.
    sim.orch_ok(
        ToolName::CancelGroup,
        json!({"group_id": "G-1", "reason": "drop"}),
    );
    sim.group();
}

#[test]
fn cancelling_a_group_removes_its_integration_worktree_after_the_tasks() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    let (_, chain) = sim.orch_ok(
        ToolName::CancelGroup,
        json!({"group_id": "G-1", "reason": "drop"}),
    );
    let git: Vec<&GitOp> = chain
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::Git(op) => Some(op),
            _ => None,
        })
        .collect();
    assert!(
        matches!(
            git[..],
            [
                GitOp::RemoveWorkspace { .. },
                GitOp::RemoveGroupWorkspace { .. }
            ]
        ),
        "{git:?}"
    );
}

/// A git whose group merge is blocked until `unblocked` is set.
fn blocked_until(unblocked: Rc<Cell<bool>>) -> GitResponder {
    Box::new(move |op| match op {
        GitOp::MergeGroup { .. } if !unblocked.get() => GitResult::Blocked {
            detail: "main worktree is dirty".into(),
        },
        other => noop_git(other),
    })
}

#[test]
fn retrying_a_blocked_merge_merges_and_tells_the_orchestrator() {
    let unblocked = Rc::new(Cell::new(false));
    let mut sim = Sim::new();
    sim.git = blocked_until(unblocked.clone());
    sim.group();
    sim.orch_ok(
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "did it"}),
    );
    sim.deliver_inbox();
    let retry = || DomainCommand::RetryGroupMerge {
        group: "G-1".into(),
    };

    // Still blocked: merge_blocked again, and the orchestrator hears about it.
    sim.run(retry()).expect("retry runs");
    assert_eq!(
        sim.state.group("G-1").map(|g| g.status),
        Some(GroupStatus::MergeBlocked)
    );
    assert_eq!(sim.inbox_kinds(), vec![InboxKind::MergeResult]);
    sim.deliver_inbox();

    unblocked.set(true);
    let chain = sim.run(retry()).expect("retry runs");
    let g = sim.state.group("G-1").expect("group");
    assert_eq!(g.status, GroupStatus::Done);
    assert_eq!(g.finish_summary.as_deref(), Some("did it"), "summary kept");
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::Git(GitOp::MergeGroup { notify: true, .. })
    )));
    let item = &sim.state.inbox.last().expect("merge_result").item;
    assert_eq!(item.kind, InboxKind::MergeResult);
    assert!(item.attrs.contains(&("ok".into(), "true".into())));
}

#[test]
fn only_a_blocked_merge_can_be_retried_and_not_while_another_group_is_open() {
    let mut sim = Sim::new();
    let retry = |g: &str| DomainCommand::RetryGroupMerge { group: g.into() };
    assert_eq!(
        sim.run(retry("G-9")).map(|_| ()).unwrap_err().code,
        ErrorCode::NotFound
    );
    sim.group();
    assert_eq!(
        sim.run(retry("G-1")).map(|_| ()).unwrap_err().code,
        ErrorCode::InvalidState
    );
    sim.git = Box::new(|op| match op {
        GitOp::MergeGroup { .. } => GitResult::Blocked {
            detail: "dirty".into(),
        },
        other => noop_git(other),
    });
    sim.orch_ok(
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "s"}),
    );
    sim.group(); // G-2 is active now
    assert_eq!(
        sim.run(retry("G-1")).map(|_| ()).unwrap_err().code,
        ErrorCode::Conflict
    );
}
