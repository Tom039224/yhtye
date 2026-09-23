//! The orchestrator's turn ends while a settled group is still open (Stage 7a):
//! a reminder first, then Yhtye finishes the group itself.

use serde_json::json;

use super::*;

impl Sim {
    fn orch_turn_ended(&mut self, outcome: TurnOutcome) -> Chain {
        self.run(DomainCommand::OrchestratorTurnEnded {
            outcome,
            prompt_queued: false,
        })
        .expect("orchestrator_turn_ended never fails")
    }

    /// G-1 with one task that is done; the `group_settled` item is delivered.
    fn settled() -> Self {
        let mut sim = Sim::new();
        sim.group();
        sim.task(json!({}));
        sim.report("T-1", Role::Implementer, None);
        assert_eq!(sim.inbox_kinds(), vec![InboxKind::GroupSettled]);
        sim.deliver_inbox();
        sim
    }

    fn group_status(&self) -> GroupStatus {
        self.state.group("G-1").expect("G-1").status
    }
}

#[test]
fn a_settled_group_left_open_gets_a_reminder_then_yhtye_finishes_it() {
    let mut sim = Sim::settled();

    // The orchestrator only talked to the user (the reported bug).
    let chain = sim.orch_turn_ended(TurnOutcome::EndTurn);
    assert!(chain.events.contains(&DomainEvent::GroupFinishReminded {
        group: "G-1".into()
    }));
    assert_eq!(sim.group_status(), GroupStatus::Active);
    assert_eq!(sim.state.group("G-1").expect("G-1").finish_nudges, 1);
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::WakeOrchestrator
    )));
    let text = sim.state.inbox[0].item.render();
    assert!(
        text.starts_with("[yhtye:group_settled] group=G-1 reminder=1\nReminder:"),
        "{text}"
    );
    assert!(text.contains("finish_group"), "{text}");

    // It ignored the reminder too: Yhtye merges the group and tells it.
    sim.deliver_inbox();
    let chain = sim.orch_turn_ended(TurnOutcome::EndTurn);
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::Git(GitOp::MergeGroup {
            trigger: MergeTrigger::Yhtye,
            ..
        })
    )));
    assert_eq!(sim.group_status(), GroupStatus::Done);
    let g = sim.state.group("G-1").expect("G-1");
    assert_eq!(
        g.finish_summary.as_deref(),
        Some(crate::prompts::AUTO_FINISH_SUMMARY)
    );
    assert_eq!(sim.inbox_kinds(), vec![InboxKind::MergeResult]);
    let text = sim.state.inbox[0].item.render();
    assert!(
        text.starts_with("[yhtye:merge_result] group=G-1 ok=true\nYhtye finished the group"),
        "{text}"
    );

    // Nothing more once the group is closed.
    sim.deliver_inbox();
    let chain = sim.orch_turn_ended(TurnOutcome::EndTurn);
    assert!(chain.events.is_empty(), "{chain:?}");
}

#[test]
fn finishing_after_the_reminder_is_the_normal_path() {
    let mut sim = Sim::settled();
    sim.orch_turn_ended(TurnOutcome::EndTurn);
    sim.deliver_inbox();
    let (reply, _) = sim.orch_ok(
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "ok"}),
    );
    assert_eq!(reply["status"], json!("done"));
    assert_eq!(
        sim.state
            .group("G-1")
            .expect("G-1")
            .finish_summary
            .as_deref(),
        Some("ok")
    );
    let chain = sim.orch_turn_ended(TurnOutcome::EndTurn);
    assert!(chain.events.is_empty(), "{chain:?}");
    assert!(
        sim.state.inbox.is_empty(),
        "no merge_result: finish_group got the reply"
    );
}

#[test]
fn no_reminder_while_work_is_open_or_the_orchestrator_has_more_to_read() {
    // Tasks still running.
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    assert!(sim.orch_turn_ended(TurnOutcome::EndTurn).events.is_empty());

    // The group_settled item is not delivered yet: the next turn reads it.
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    sim.report("T-1", Role::Implementer, None);
    assert!(sim.orch_turn_ended(TurnOutcome::EndTurn).events.is_empty());

    // A cancelled turn, or one with another prompt queued.
    let mut sim = Sim::settled();
    assert!(
        sim.orch_turn_ended(TurnOutcome::Cancelled)
            .events
            .is_empty()
    );
    let chain = sim
        .run(DomainCommand::OrchestratorTurnEnded {
            outcome: TurnOutcome::EndTurn,
            prompt_queued: true,
        })
        .expect("ok");
    assert!(chain.events.is_empty());

    // An empty group (no tasks yet) is not settled.
    let mut sim = Sim::new();
    sim.group();
    assert!(sim.orch_turn_ended(TurnOutcome::EndTurn).events.is_empty());
}

#[test]
fn tasks_that_cannot_start_get_a_reminder_but_no_automatic_finish() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    sim.task(json!({"depends_on": ["T-1"]}));
    sim.orch_ok(
        ToolName::CancelTask,
        json!({"task_id": "T-1", "reason": "x"}),
    );
    sim.deliver_inbox();
    let chain = sim.orch_turn_ended(TurnOutcome::EndTurn);
    assert!(chain.events.contains(&DomainEvent::GroupFinishReminded {
        group: "G-1".into()
    }));
    sim.deliver_inbox();
    let chain = sim.orch_turn_ended(TurnOutcome::EndTurn);
    assert!(
        chain.events.is_empty(),
        "finish_group would refuse: T-2 is pending"
    );
    assert_eq!(sim.group_status(), GroupStatus::Active);
}

#[test]
fn a_blocked_automatic_merge_is_reported_as_merge_result() {
    let mut sim = Sim::settled();
    sim.git = Box::new(|op| match op {
        GitOp::MergeGroup { .. } => GitResult::Dirty {
            files: vec!["a.txt".into()],
        },
        other => noop_git(other),
    });
    sim.orch_turn_ended(TurnOutcome::EndTurn);
    sim.deliver_inbox();
    sim.orch_turn_ended(TurnOutcome::EndTurn);
    assert_eq!(sim.group_status(), GroupStatus::MergeBlocked);
    let text = sim.state.inbox[0].item.render();
    assert!(
        text.starts_with("[yhtye:merge_result] group=G-1 ok=false"),
        "{text}"
    );
    assert!(text.contains("a.txt"), "{text}");
}

#[test]
fn groups_logged_before_the_counter_existed_start_at_zero() {
    let old = json!({"type": "group_created", "group": {
        "id": "G-1", "title": "g", "summary": null, "base_branch": "main",
        "group_branch": "yhtye/G-1", "status": "active", "finish_summary": null, "detail": null}});
    let event: DomainEvent = serde_json::from_value(old).expect("old event parses");
    let mut state = State::new("P-1", DomainConfig::default());
    state.apply(&event);
    assert_eq!(state.group("G-1").expect("G-1").finish_nudges, 0);
}
