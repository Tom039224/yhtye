//! Chats (`orchestration-model.md` §2.0, Stage 8 / 8e): creating them, titles,
//! the open-group limit per worktree, the base branch a group records and
//! `finish_group{into}`, ownership of groups and tasks, and inbox routing.

use serde_json::json;

use super::*;

/// Two chats: `C-1` in `/wt/main` (from [`Sim::new`]) and `C-2` in `/wt/feature`.
fn two_chats() -> Sim {
    let mut sim = Sim::new();
    assert_eq!(sim.chat("feature"), "C-2");
    sim
}

/// `C-2` creates a group (`G-2` when `G-1` exists) and returns the reply.
fn group_of_c2(sim: &mut Sim) -> Result<Chain, ToolError> {
    sim.tool(
        orch_of("C-2"),
        ToolName::CreateGroup,
        json!({"title": "other"}),
    )
}

fn user_message(sim: &mut Sim, chat: &str, text: &str) -> Result<Chain, ToolError> {
    sim.run(DomainCommand::UserMessage {
        chat: chat.into(),
        text: text.into(),
    })
}

#[test]
fn chats_are_numbered_per_project_and_keep_their_worktree() {
    let sim = two_chats();
    let chats: Vec<(&str, PathBuf, Option<&str>)> = sim
        .state
        .chats
        .iter()
        .map(|c| (c.id.as_str(), c.worktree.clone(), c.title.as_deref()))
        .collect();
    assert_eq!(
        chats,
        [("C-1", wt("main"), None), ("C-2", wt("feature"), None)]
    );
    assert_eq!(sim.state.counters.chats, 2);
}

#[test]
fn a_chat_needs_a_worktree() {
    let mut sim = Sim::new();
    let err = sim
        .run(DomainCommand::CreateChat {
            worktree: PathBuf::new(),
        })
        .expect_err("refused");
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert_eq!(sim.state.chats.len(), 1, "nothing was created");
}

#[test]
fn the_first_message_titles_the_chat_once() {
    let mut sim = two_chats();
    let chain = user_message(
        &mut sim,
        "C-1",
        "  add a\nline to the README, please, and then some more words  ",
    )
    .expect("ok");
    let title = chain.events.iter().find_map(|e| match e {
        DomainEvent::ChatTitled { chat, title } => Some((chat.as_str(), title.as_str())),
        _ => None,
    });
    assert_eq!(
        title,
        Some(("C-1", "add a line to the README, please, and th"))
    );
    assert_eq!(title.map(|(_, t)| t.chars().count()), Some(MAX_TITLE_CHARS));
    let again = user_message(&mut sim, "C-1", "second message").expect("ok");
    assert!(
        !again
            .events
            .iter()
            .any(|e| matches!(e, DomainEvent::ChatTitled { .. })),
        "titled once"
    );
    assert_eq!(sim.state.chat("C-2").expect("C-2").title, None);
}

#[test]
fn a_message_to_an_unknown_chat_is_not_found() {
    let mut sim = Sim::new();
    let err = user_message(&mut sim, "C-9", "hi").expect_err("unknown chat");
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(sim.state.inbox.is_empty());
}

#[test]
fn a_group_merges_into_the_branch_its_worktree_has_checked_out() {
    let mut sim = two_chats();
    sim.group();
    group_of_c2(&mut sim).expect("C-2 creates a group");
    let bases: Vec<(&str, &str, &str)> = sim
        .state
        .groups
        .iter()
        .map(|g| (g.id.as_str(), g.chat.as_str(), g.base_branch.as_str()))
        .collect();
    assert_eq!(bases, [("G-1", "C-1", "main"), ("G-2", "C-2", "feature")]);
}

#[test]
fn chats_in_different_worktrees_have_open_groups_at_once() {
    let mut sim = two_chats();
    sim.group();
    let chain = group_of_c2(&mut sim).expect("no conflict across worktrees");
    let reply = chain.reply.expect("reply").expect("ok");
    assert_eq!(reply["base_branch"], json!("feature"));
    assert!(sim.state.open_group_of("C-1").is_some() && sim.state.open_group_of("C-2").is_some());
}

#[test]
fn a_second_open_group_in_the_same_worktree_conflicts_even_across_chats() {
    let mut sim = Sim::new();
    sim.chat("main"); // C-2 in the same worktree
    sim.group();
    let err = sim
        .tool(
            orch_of("C-2"),
            ToolName::CreateGroup,
            json!({"title": "again"}),
        )
        .expect_err("same worktree");
    assert_eq!(err.code, ErrorCode::Conflict);
    assert!(
        err.message.contains("G-1") && err.message.contains("C-1"),
        "{err}"
    );
    // The chat's own second group conflicts too (as before Stage 8).
    assert_eq!(
        sim.orch_err(ToolName::CreateGroup, json!({"title": "again"})),
        ErrorCode::Conflict
    );
    // Once the group is cancelled the worktree is free again.
    sim.orch_ok(
        ToolName::CancelGroup,
        json!({"group_id": "G-1", "reason": "x"}),
    );
    sim.tool(
        orch_of("C-2"),
        ToolName::CreateGroup,
        json!({"title": "now"}),
    )
    .expect("free again");
}

#[test]
fn a_blocked_merge_does_not_count_and_retry_respects_the_worktree_limit() {
    let mut sim = Sim::new();
    sim.chat("main");
    sim.git = Box::new(|op| match op {
        GitOp::MergeGroup { .. } => GitResult::Blocked {
            detail: "dirty".into(),
        },
        other => noop_git(other),
    });
    sim.group();
    sim.orch_ok(
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "s"}),
    );
    assert_eq!(
        sim.state.group("G-1").expect("G-1").status,
        GroupStatus::MergeBlocked
    );
    sim.tool(
        orch_of("C-2"),
        ToolName::CreateGroup,
        json!({"title": "next"}),
    )
    .expect("a merge_blocked group does not count");
    let err = sim
        .run(DomainCommand::RetryGroupMerge {
            group: "G-1".into(),
        })
        .expect_err("G-2 is open in the worktree");
    assert_eq!(err.code, ErrorCode::Conflict);
}

#[test]
fn tools_only_reach_the_orchestrators_own_chat() {
    let mut sim = two_chats();
    sim.group();
    sim.task(json!({}));
    let c2 = orch_of("C-2");
    let calls = [
        (
            ToolName::CreateTask,
            json!({"group_id": "G-1", "title": "t", "kind": "code",
            "steps": [{"kind": "implement"}], "instruction": "x"}),
        ),
        (
            ToolName::SetInstruction,
            json!({"task_id": "T-1", "instruction": "x"}),
        ),
        (
            ToolName::ModifySteps,
            json!({"task_id": "T-1", "steps": [{"kind": "implement"}]}),
        ),
        (
            ToolName::ResolveCheckpoint,
            json!({"task_id": "T-1", "decision": "continue"}),
        ),
        (
            ToolName::CancelTask,
            json!({"task_id": "T-1", "reason": "x"}),
        ),
        (
            ToolName::FinishGroup,
            json!({"group_id": "G-1", "summary": "x"}),
        ),
        (
            ToolName::CancelGroup,
            json!({"group_id": "G-1", "reason": "x"}),
        ),
        (ToolName::GetStatus, json!({"group_id": "G-1"})),
        (ToolName::GetTask, json!({"task_id": "T-1"})),
    ];
    for (tool, args) in calls {
        let before = sim.state.clone();
        let err = sim.tool(c2.clone(), tool, args).expect_err("forbidden");
        assert_eq!(err.code, ErrorCode::Forbidden, "{tool:?}: {err}");
        assert_eq!(before, sim.state, "{tool:?} changed something");
    }
    // Unknown ids are still `not_found`, and the owner is not restricted.
    let err = sim
        .tool(c2, ToolName::GetTask, json!({"task_id": "T-9"}))
        .expect_err("unknown");
    assert_eq!(err.code, ErrorCode::NotFound);
    sim.orch_ok(ToolName::GetTask, json!({"task_id": "T-1"}));
}

#[test]
fn a_help_can_only_be_answered_by_the_chat_that_owns_the_task() {
    let mut sim = two_chats();
    sim.group();
    sim.task(json!({}));
    sim.sub_ok(
        sub("T-1", Role::Implementer, 0),
        ToolName::Help,
        json!({"kind": "question", "message": "which file?"}),
    );
    let err = sim
        .tool(
            orch_of("C-2"),
            ToolName::AnswerHelp,
            json!({"help_id": "H-1", "action": "cancel_task"}),
        )
        .expect_err("not theirs");
    assert_eq!(err.code, ErrorCode::Forbidden);
    sim.orch_ok(
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "cancel_task"}),
    );
}

#[test]
fn get_status_without_a_group_shows_the_own_chats_group_only() {
    let mut sim = two_chats();
    sim.group(); // G-1 of C-1
    let err = sim
        .tool(orch_of("C-2"), ToolName::GetStatus, json!({}))
        .expect_err("C-2 has no group");
    assert_eq!(err.code, ErrorCode::NotFound);
    group_of_c2(&mut sim).expect("G-2");
    let chain = sim
        .tool(orch_of("C-2"), ToolName::GetStatus, json!({}))
        .expect("status");
    let status = chain.reply.expect("reply").expect("ok");
    assert_eq!(status["group"]["id"], json!("G-2"));
    let (status, _) = sim.orch_ok(ToolName::GetStatus, json!({}));
    assert_eq!(status["group"]["id"], json!("G-1"));
}

#[test]
fn notifications_go_to_the_chat_that_owns_the_group() {
    let mut sim = two_chats();
    sim.group(); // G-1 of C-1
    group_of_c2(&mut sim).expect("G-2 of C-2");
    let c2 = orch_of("C-2");
    sim.tool(
        c2,
        ToolName::CreateTask,
        json!({"group_id": "G-2", "title": "t", "kind": "code",
               "steps": [{"kind": "implement"}], "instruction": "x"}),
    )
    .expect("T-1 in G-2");
    // The agent asks for help, then finishes another task.
    sim.sub_ok(
        SessionBinding {
            group: Some("G-2".into()),
            ..sub("T-1", Role::Implementer, 0)
        },
        ToolName::Help,
        json!({"kind": "question", "message": "which file?"}),
    );
    assert_eq!(sim.inbox_kinds_of("C-2"), vec![InboxKind::HelpRaised]);
    assert!(sim.inbox_kinds_of("C-1").is_empty(), "C-1 hears nothing");
    // A user message reaches its own chat only; delivering one chat's inbox
    // leaves the other's.
    user_message(&mut sim, "C-1", "hi").expect("ok");
    assert_eq!(sim.inbox_kinds_of("C-1"), vec![InboxKind::UserMessage]);
    sim.deliver_inbox_of("C-2");
    assert!(sim.inbox_kinds_of("C-2").is_empty());
    assert_eq!(sim.inbox_kinds_of("C-1"), vec![InboxKind::UserMessage]);
    // Everything queued for a chat wakes that chat's orchestrator.
    let chain = user_message(&mut sim, "C-2", "hello").expect("ok");
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::WakeOrchestrator { chat } if chat == "C-2"
    )));
}

#[test]
fn a_users_cancel_is_reported_to_the_groups_chat() {
    let mut sim = two_chats();
    sim.group();
    group_of_c2(&mut sim).expect("G-2");
    sim.tool(
        orch_of("C-2"),
        ToolName::CreateTask,
        json!({"group_id": "G-2", "title": "t", "kind": "code",
               "steps": [{"kind": "implement"}], "instruction": "x"}),
    )
    .expect("T-1 in G-2");
    sim.deliver_inbox_of("C-1");
    sim.deliver_inbox_of("C-2");
    // The cancelled group's settle notice goes to the chat that owns it.
    let user = SessionBinding {
        chat: None,
        ..orch()
    };
    let call = crate::mcp::tools::parse_call(
        ToolName::CancelTask,
        json!({"task_id": "T-1", "reason": "user"})
            .as_object()
            .cloned(),
    )
    .expect("call");
    sim.run(DomainCommand::Tool {
        binding: user,
        call,
    })
    .expect("user cancels");
    assert_eq!(sim.inbox_kinds_of("C-2"), vec![InboxKind::GroupSettled]);
    assert!(sim.inbox_kinds_of("C-1").is_empty());
}

#[test]
fn a_turn_end_only_reminds_about_the_chats_own_settled_group() {
    let mut sim = two_chats();
    sim.group(); // G-1 of C-1, settled below
    sim.task(json!({}));
    sim.report("T-1", Role::Implementer, None);
    sim.deliver_inbox_of("C-1");
    let other = sim
        .run(DomainCommand::OrchestratorTurnEnded {
            chat: "C-2".into(),
            outcome: TurnOutcome::EndTurn,
            prompt_queued: false,
        })
        .expect("ok");
    assert!(
        other.events.is_empty(),
        "C-2's turn end says nothing about G-1"
    );
    let own = sim
        .run(DomainCommand::OrchestratorTurnEnded {
            chat: "C-1".into(),
            outcome: TurnOutcome::EndTurn,
            prompt_queued: false,
        })
        .expect("ok");
    assert!(own.events.contains(&DomainEvent::GroupFinishReminded {
        group: "G-1".into()
    }));
}

#[test]
fn restart_tells_each_listed_chat_what_it_lost_and_only_its_own_state() {
    let mut sim = two_chats();
    sim.group();
    sim.task(json!({}));
    group_of_c2(&mut sim).expect("G-2");
    sim.deliver_inbox_of("C-1");
    let lost = OrchestratorResume {
        had_session: true,
        restored: false,
        turn_was_running: false,
    };
    sim.run(DomainCommand::Restart {
        orchestrators: vec![("C-2".into(), lost)],
    })
    .expect("restart");
    assert!(sim.inbox_kinds_of("C-1").is_empty(), "C-1 was not restored");
    assert_eq!(sim.inbox_kinds_of("C-2"), vec![InboxKind::Restarted]);
    let text = sim
        .state
        .inbox_of("C-2")
        .next()
        .expect("entry")
        .item
        .body
        .clone();
    assert!(text.contains("group G-2"), "{text}");
    assert!(!text.contains("G-1") && !text.contains("T-1"), "{text}");
}

#[test]
fn a_started_again_orchestrator_is_told_when_its_turn_was_cut_off() {
    let mut sim = two_chats();
    let fine = OrchestratorResume {
        had_session: true,
        restored: true,
        turn_was_running: false,
    };
    let chain = sim
        .run(DomainCommand::TellOrchestrator {
            chat: "C-1".into(),
            resume: fine,
        })
        .expect("ok");
    assert!(chain.events.is_empty(), "nothing was missed");
    let cut = OrchestratorResume {
        turn_was_running: true,
        ..fine
    };
    sim.run(DomainCommand::TellOrchestrator {
        chat: "C-1".into(),
        resume: cut,
    })
    .expect("ok");
    let item = sim
        .state
        .inbox_of("C-1")
        .next()
        .expect("entry")
        .item
        .render();
    assert!(
        item.starts_with("[yhtye:restarted]") && item.contains("cut off"),
        "{item}"
    );
}

fn blocked_merges(sim: &mut Sim) {
    sim.git = Box::new(|op| match op {
        GitOp::MergeGroup { .. } => GitResult::Blocked {
            detail: "the worktree is on another branch".into(),
        },
        other => noop_git(other),
    });
}

/// `(worktree, base_branch)` of every `MergeGroup` effect in `chain`.
fn merges(chain: &Chain) -> Vec<(PathBuf, String)> {
    chain
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::Git(GitOp::MergeGroup {
                worktree,
                base_branch,
                ..
            }) => Some((worktree.clone(), base_branch.clone())),
            _ => None,
        })
        .collect()
}

fn finish(sim: &mut Sim, args: Value) -> Result<Chain, ToolError> {
    sim.tool(orch(), ToolName::FinishGroup, args)
}

#[test]
fn a_group_records_the_branch_checked_out_when_it_is_created() {
    let mut sim = Sim::new();
    sim.checkout("main", Some("trunk"));
    sim.group();
    assert_eq!(sim.state.group("G-1").expect("G-1").base_branch, "trunk");
    // A later checkout does not change the recorded base branch.
    sim.checkout("main", Some("other"));
    assert_eq!(sim.state.group("G-1").expect("G-1").base_branch, "trunk");
    let chain = finish(&mut sim, json!({"group_id": "G-1", "summary": "s"})).expect("finish");
    assert_eq!(merges(&chain), [(wt("main"), "trunk".to_string())]);
}

#[test]
fn a_group_needs_a_branch_checked_out() {
    let mut sim = Sim::new();
    for head in [None, Some("yhtye/G-7")] {
        sim.checkout("main", head);
        assert_eq!(
            sim.orch_err(ToolName::CreateGroup, json!({"title": "g"})),
            ErrorCode::InvalidState,
            "{head:?}"
        );
    }
}

#[test]
fn the_open_group_limit_is_per_worktree_whatever_the_branch() {
    let mut sim = two_chats();
    sim.group(); // G-1 in /wt/main on main
    sim.checkout("main", Some("renamed"));
    assert_eq!(
        sim.orch_err(ToolName::CreateGroup, json!({"title": "again"})),
        ErrorCode::Conflict,
        "the same worktree, now on another branch"
    );
    // Another worktree, even with the same branch name checked out (git would
    // not allow it, but the limit does not look at names).
    sim.checkout("feature", Some("main"));
    group_of_c2(&mut sim).expect("another worktree");
}

#[test]
fn finish_group_into_moves_the_base_branch_then_merges() {
    let mut sim = Sim::new();
    sim.group();
    let chain = finish(
        &mut sim,
        json!({"group_id": "G-1", "summary": "s", "into": "trunk"}),
    )
    .expect("finish");
    assert!(chain.events.contains(&DomainEvent::GroupBaseChanged {
        group: "G-1".into(),
        from: "main".into(),
        to: "trunk".into(),
    }));
    assert_eq!(merges(&chain), [(wt("main"), "trunk".to_string())]);
    let g = sim.state.group("G-1").expect("G-1");
    assert_eq!(
        (g.base_branch.as_str(), g.status),
        ("trunk", GroupStatus::Done)
    );
}

#[test]
fn finish_group_into_the_same_branch_changes_nothing_and_bad_names_are_refused() {
    let mut sim = Sim::new();
    sim.group();
    for into in [" ", "yhtye/G-1"] {
        let args = json!({"group_id": "G-1", "summary": "s", "into": into});
        assert_eq!(
            sim.orch_err(ToolName::FinishGroup, args),
            ErrorCode::InvalidArgument,
            "{into:?}"
        );
    }
    let chain = finish(
        &mut sim,
        json!({"group_id": "G-1", "summary": "s", "into": "main"}),
    )
    .expect("finish");
    assert!(
        !chain
            .events
            .iter()
            .any(|e| matches!(e, DomainEvent::GroupBaseChanged { .. })),
        "no change"
    );
}

#[test]
fn finish_group_runs_the_merge_of_a_blocked_group_again() {
    let mut sim = Sim::new();
    blocked_merges(&mut sim);
    sim.group();
    let (reply, _) = sim.orch_ok(
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "s"}),
    );
    assert_eq!(reply["merge"]["ok"], json!(false));
    assert_eq!(reply["status"], json!("merge_blocked"));
    assert!(
        sim.state.inbox_of("C-1").next().is_none(),
        "the orchestrator's own finish_group gets the reply, not an inbox item"
    );
    sim.git = Box::new(noop_git);
    let chain = finish(
        &mut sim,
        json!({"group_id": "G-1", "summary": "again", "into": "trunk"}),
    )
    .expect("again");
    assert_eq!(merges(&chain), [(wt("main"), "trunk".to_string())]);
    let g = sim.state.group("G-1").expect("G-1");
    assert_eq!(g.status, GroupStatus::Done);
    assert_eq!(g.finish_summary.as_deref(), Some("again"));
    // A finished group cannot be finished again.
    assert_eq!(
        sim.orch_err(
            ToolName::FinishGroup,
            json!({"group_id": "G-1", "summary": "s"})
        ),
        ErrorCode::InvalidState
    );
}

#[test]
fn finish_group_of_a_blocked_group_respects_the_worktree_limit() {
    let mut sim = Sim::new();
    blocked_merges(&mut sim);
    sim.group();
    finish(&mut sim, json!({"group_id": "G-1", "summary": "s"})).expect("blocked");
    sim.group(); // G-2, active in the same worktree
    assert_eq!(
        sim.orch_err(
            ToolName::FinishGroup,
            json!({"group_id": "G-1", "summary": "s"})
        ),
        ErrorCode::Conflict
    );
}

#[test]
fn a_merge_yhtye_or_the_user_started_is_reported_to_the_chat() {
    let mut sim = Sim::new();
    blocked_merges(&mut sim);
    sim.group();
    finish(&mut sim, json!({"group_id": "G-1", "summary": "s"})).expect("blocked");
    sim.run(DomainCommand::RetryGroupMerge {
        group: "G-1".into(),
    })
    .expect("retried");
    let item = sim.state.inbox_of("C-1").last().expect("merge_result");
    assert_eq!(item.item.kind, InboxKind::MergeResult);
    assert!(
        item.item.render().contains("another branch"),
        "{}",
        item.item.render()
    );
}
