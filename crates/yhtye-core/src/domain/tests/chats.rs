//! Chats (`orchestration-model.md` §2.0, Stage 8): creating them, titles, the
//! open-group limit per branch, ownership of groups and tasks, and inbox routing.

use serde_json::json;

use super::*;

/// Two chats: `C-1` on `main` (from [`Sim::new`]) and `C-2` on `feature`.
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
fn chats_are_numbered_per_project_and_keep_their_branch() {
    let sim = two_chats();
    let chats: Vec<(&str, &str, Option<&str>)> = sim
        .state
        .chats
        .iter()
        .map(|c| (c.id.as_str(), c.branch.as_str(), c.title.as_deref()))
        .collect();
    assert_eq!(chats, [("C-1", "main", None), ("C-2", "feature", None)]);
    assert_eq!(sim.state.counters.chats, 2);
}

#[test]
fn a_chat_needs_a_real_branch_that_is_not_internal() {
    let mut sim = Sim::new();
    for (branch, code) in [
        (" ", ErrorCode::InvalidArgument),
        ("yhtye/G-1", ErrorCode::InvalidArgument),
    ] {
        let err = sim
            .run(DomainCommand::CreateChat {
                branch: branch.into(),
            })
            .expect_err("refused");
        assert_eq!(err.code, code, "{branch:?}");
    }
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
fn a_group_merges_into_its_chats_branch() {
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
fn chats_on_different_branches_have_open_groups_at_once() {
    let mut sim = two_chats();
    sim.group();
    let chain = group_of_c2(&mut sim).expect("no conflict across branches");
    let reply = chain.reply.expect("reply").expect("ok");
    assert_eq!(reply["base_branch"], json!("feature"));
    assert!(sim.state.open_group_of("C-1").is_some() && sim.state.open_group_of("C-2").is_some());
}

#[test]
fn a_second_open_group_on_the_same_branch_conflicts_even_across_chats() {
    let mut sim = Sim::new();
    sim.chat("main"); // C-2 on the same branch
    sim.group();
    let err = sim
        .tool(
            orch_of("C-2"),
            ToolName::CreateGroup,
            json!({"title": "again"}),
        )
        .expect_err("same branch");
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
    // Once the group is cancelled the branch is free again.
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
fn a_blocked_merge_does_not_count_and_retry_respects_the_branch_limit() {
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
        .expect_err("G-2 is open on the branch");
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

fn rename(sim: &mut Sim, chat: &str, to: &str) -> Result<Chain, ToolError> {
    sim.run(DomainCommand::ChatBranchChanged {
        chat: chat.into(),
        to: to.into(),
    })
}

#[test]
fn a_renamed_branch_moves_the_chat_and_its_unfinished_groups() {
    let mut sim = two_chats();
    sim.git = Box::new(|op| match op {
        GitOp::MergeGroup { .. } => GitResult::Blocked {
            detail: "dirty".into(),
        },
        other => noop_git(other),
    });
    sim.group(); // G-1 of C-1, blocked by the merge below
    sim.orch_ok(
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "s"}),
    );
    sim.orch_ok(ToolName::CreateGroup, json!({"title": "second"})); // G-2, active
    group_of_c2(&mut sim).expect("G-3 of C-2 on feature");
    let chain = rename(&mut sim, "C-1", "main-2").expect("renamed");
    assert_eq!(
        chain.events,
        [DomainEvent::ChatBranchChanged {
            chat: "C-1".into(),
            from: "main".into(),
            to: "main-2".into(),
        }]
    );
    assert_eq!(sim.state.chat("C-1").expect("C-1").branch, "main-2");
    let bases: Vec<(&str, &str)> = sim
        .state
        .groups
        .iter()
        .map(|g| (g.id.as_str(), g.base_branch.as_str()))
        .collect();
    assert_eq!(
        bases,
        [("G-1", "main-2"), ("G-2", "main-2"), ("G-3", "feature")],
        "the blocked and the active group follow, the other chat's does not"
    );
    assert_eq!(sim.state.chat("C-2").expect("C-2").branch, "feature");
}

#[test]
fn finished_groups_keep_the_branch_they_were_merged_into() {
    let mut sim = Sim::new();
    sim.group();
    sim.orch_ok(
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "s"}),
    );
    assert_eq!(
        sim.state.group("G-1").expect("G-1").status,
        GroupStatus::Done
    );
    rename(&mut sim, "C-1", "trunk").expect("renamed");
    assert_eq!(sim.state.group("G-1").expect("G-1").base_branch, "main");
}

#[test]
fn the_group_after_a_rename_merges_into_the_new_branch() {
    let mut sim = Sim::new();
    rename(&mut sim, "C-1", "trunk").expect("renamed");
    sim.group();
    let chain = sim
        .tool(
            orch(),
            ToolName::FinishGroup,
            json!({"group_id": "G-1", "summary": "s"}),
        )
        .expect("finished");
    let merged_into: Vec<&str> = chain
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::Git(GitOp::MergeGroup { base_branch, .. }) => Some(base_branch.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(merged_into, ["trunk"]);
}

#[test]
fn a_rename_counts_for_the_open_group_limit_of_the_new_branch() {
    let mut sim = two_chats();
    sim.group(); // G-1 of C-1 on main
    rename(&mut sim, "C-1", "feature").expect("renamed onto C-2's branch");
    let err = group_of_c2(&mut sim).expect_err("G-1 is now open on feature");
    assert_eq!(err.code, ErrorCode::Conflict);
}

#[test]
fn a_rename_needs_a_known_chat_and_a_valid_branch() {
    let mut sim = Sim::new();
    let before = sim.state.clone();
    for (chat, to, code) in [
        ("C-9", "x", ErrorCode::NotFound),
        ("C-1", " ", ErrorCode::InvalidArgument),
        ("C-1", "yhtye/G-1", ErrorCode::InvalidArgument),
    ] {
        let err = rename(&mut sim, chat, to).expect_err("refused");
        assert_eq!(err.code, code, "{chat} {to:?}");
    }
    assert_eq!(sim.state, before);
    let same = rename(&mut sim, "C-1", "main").expect("a no-op");
    assert!(
        same.events.is_empty(),
        "renaming to the same name is a no-op"
    );
}
