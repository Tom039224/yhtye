//! Turn ends without a report, stop reasons, crashes and start failures
//! (`orchestration-model.md` §7).

use serde_json::json;

use super::*;

fn running() -> Sim {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    sim.deliver_inbox();
    sim
}

fn open_help(sim: &Sim) -> Help {
    sim.state.open_help("T-1").expect("open help").clone()
}

#[test]
fn silent_turn_gets_one_reminder_then_protocol_violation() {
    let mut sim = running();
    let me = agent("T-1", Role::Implementer, 0);
    let chain = sim.turn_ended(me.clone(), TurnOutcome::EndTurn);
    assert_eq!(chain.effects.len(), 1);
    assert!(prompt_of(&chain).starts_with("[yhtye:reminder] task=T-1 step=1"));
    assert_eq!(sim.t("T-1").steps[0].nudges, 1);
    assert_eq!(sim.status("T-1"), TaskStatus::Running);

    sim.turn_ended(me, TurnOutcome::EndTurn);
    assert_eq!(sim.status("T-1"), TaskStatus::Handling);
    let help = open_help(&sim);
    assert_eq!(help.kind, HelpKind::ProtocolViolation);
    assert_eq!(sim.inbox_kinds(), vec![InboxKind::HelpRaised]);
    assert!(
        sim.state.inbox[0]
            .item
            .render()
            .starts_with("[yhtye:help_raised] help_id=H-1 task=T-1 kind=protocol_violation\n")
    );

    // Resume restarts the step with a fresh prompt; the reply becomes a note.
    let (_, chain) = sim.orch_ok(
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "resume", "reply": "call report_step_done"}),
    );
    assert_eq!(
        run_steps(&chain),
        vec![("T-1".into(), Role::Implementer, 0)]
    );
    assert!(prompt_of(&chain).contains("Note from the orchestrator:\ncall report_step_done"));
    assert_eq!(sim.t("T-1").steps[0].nudges, 0, "fresh reminder budget");
}

#[test]
fn reminder_then_report_continues_normally() {
    let mut sim = running();
    sim.turn_ended(agent("T-1", Role::Implementer, 0), TurnOutcome::EndTurn);
    sim.report("T-1", Role::Implementer, None);
    assert_eq!(sim.status("T-1"), TaskStatus::Done);
}

#[test]
fn stop_reasons_raise_agent_stopped() {
    for outcome in [
        TurnOutcome::Stopped("max_tokens".into()),
        TurnOutcome::Error("boom".into()),
    ] {
        let mut sim = running();
        sim.turn_ended(agent("T-1", Role::Implementer, 0), outcome);
        assert_eq!(open_help(&sim).kind, HelpKind::AgentStopped);
    }
}

#[test]
fn cancelled_and_closed_turns_change_nothing() {
    for outcome in [TurnOutcome::Cancelled, TurnOutcome::Closed] {
        let mut sim = running();
        let before = sim.state.clone();
        let chain = sim.turn_ended(agent("T-1", Role::Implementer, 0), outcome);
        assert!(chain.events.is_empty() && chain.effects.is_empty());
        assert_eq!(before, sim.state);
    }
}

#[test]
fn crash_mid_step_raises_agent_crashed_and_resume_restarts_the_step() {
    let mut sim = running();
    let chain = sim
        .run(DomainCommand::AgentExited {
            agent: agent("T-1", Role::Implementer, 0),
            detail: "exit code Some(3)".into(),
        })
        .expect("ok");
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::WakeOrchestrator
    )));
    let help = open_help(&sim);
    assert_eq!(help.kind, HelpKind::AgentCrashed);
    assert!(help.message.contains("exit code Some(3)"));
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
fn crash_while_waiting_for_an_answer_restarts_instead_of_prompting() {
    let mut sim = running();
    sim.sub_ok(
        sub("T-1", Role::Implementer, 0),
        ToolName::Help,
        json!({"kind": "question", "message": "?"}),
    );
    sim.run(DomainCommand::AgentExited {
        agent: agent("T-1", Role::Implementer, 0),
        detail: "killed".into(),
    })
    .expect("ok");
    assert!(open_help(&sim).agent_lost);
    // No reply needed any more: the step is restarted in a new session.
    let (_, chain) = sim.orch_ok(
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "resume", "reply": "use a.txt"}),
    );
    assert_eq!(
        run_steps(&chain),
        vec![("T-1".into(), Role::Implementer, 0)]
    );
    assert!(prompt_of(&chain).contains("Note from the orchestrator:\nuse a.txt"));
}

#[test]
fn start_failure_raises_agent_crashed() {
    let mut sim = running();
    sim.run(DomainCommand::AgentStartFailed {
        agent: agent("T-1", Role::Implementer, 0),
        error: "npx not found".into(),
    })
    .expect("ok");
    let help = open_help(&sim);
    assert_eq!(help.kind, HelpKind::AgentCrashed);
    assert!(help.message.contains("npx not found"));
}

#[test]
fn stale_sessions_are_ignored() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({"steps": [{"kind": "implement"}, {"kind": "review"}]}));
    sim.report("T-1", Role::Implementer, None);
    sim.deliver_inbox();
    let before = sim.state.clone();
    // The implementer is idle while the review runs: its exit and turn ends
    // do not concern the current step.
    let old = agent("T-1", Role::Implementer, 0);
    sim.turn_ended(old.clone(), TurnOutcome::EndTurn);
    sim.run(DomainCommand::AgentExited {
        agent: old,
        detail: "x".into(),
    })
    .expect("ok");
    sim.run(DomainCommand::TurnEnded {
        agent: agent("T-9", Role::Implementer, 0),
        outcome: TurnOutcome::EndTurn,
        prompt_queued: false,
    })
    .expect("unknown task is ignored");
    assert_eq!(before, sim.state);
}

#[test]
fn events_after_the_task_ended_are_ignored() {
    let mut sim = running();
    sim.orch_ok(
        ToolName::CancelTask,
        json!({"task_id": "T-1", "reason": "x"}),
    );
    let before = sim.state.clone();
    let me = agent("T-1", Role::Implementer, 0);
    sim.turn_ended(me.clone(), TurnOutcome::Cancelled);
    sim.turn_ended(me.clone(), TurnOutcome::EndTurn);
    sim.run(DomainCommand::AgentExited {
        agent: me,
        detail: "x".into(),
    })
    .expect("ok");
    assert_eq!(before, sim.state);
}

#[test]
fn help_answered_before_the_turn_ends_is_not_a_silent_turn() {
    let mut sim = running();
    sim.sub_ok(
        sub("T-1", Role::Implementer, 0),
        ToolName::Help,
        json!({"kind": "question", "message": "?"}),
    );
    // The orchestrator answers while the help-raising turn is still running.
    sim.orch_ok(
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "resume", "reply": "go on"}),
    );
    let chain = sim
        .run(DomainCommand::TurnEnded {
            agent: agent("T-1", Role::Implementer, 0),
            outcome: TurnOutcome::EndTurn,
            prompt_queued: true,
        })
        .expect("ok");
    assert!(chain.effects.is_empty(), "no reminder: {:?}", chain.effects);
    assert_eq!(sim.t("T-1").steps[0].nudges, 0);
}
