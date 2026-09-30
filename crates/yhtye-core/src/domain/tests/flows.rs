//! Whole task lifecycles through the state machine.

use serde_json::json;

use super::*;

#[test]
fn simple_task_runs_merges_and_settles_the_group() {
    let mut sim = Sim::new();
    sim.group();
    let chain = sim.task(json!({}));
    assert_eq!(
        run_steps(&chain),
        vec![("T-1".into(), Role::Implementer, 0)]
    );
    assert!(has_effect(&chain, |e| matches!(e,
        Effect::RunStep { workdir: Some(w), .. } if w.to_str() == Some("/wt/T-1"))));
    assert!(prompt_of(&chain).starts_with("[yhtye:step] task=T-1 kind=code step=1/2"));
    assert_eq!(sim.status("T-1"), TaskStatus::Running);

    let chain = sim.report("T-1", Role::Implementer, None);
    assert_eq!(sim.status("T-1"), TaskStatus::Done);
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::Git(GitOp::FinishTask { .. })
    )));
    assert!(has_effect(
        &chain,
        |e| matches!(e, Effect::StopTaskAgents { task } if task == "T-1")
    ));
    assert_eq!(sim.inbox_kinds(), vec![InboxKind::GroupSettled]);
    assert_eq!(
        sim.state.inbox[0].item.render(),
        "[yhtye:group_settled] group=G-1\nT-1 done: result of step 0"
    );

    let (reply, _) = sim.orch_ok(
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "ok"}),
    );
    assert_eq!(reply["status"], json!("done"));
    assert_eq!(reply["merge"]["ok"], json!(true));
    // The group is closed, so a new one may start.
    sim.group();
    assert_eq!(sim.state.groups.len(), 2);
}

#[test]
fn the_next_step_starts_only_when_the_turn_ends() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({"steps": [{"kind": "implement"}, {"kind": "implement"}]}));
    let chain = sim.sub_ok(
        sub("T-1", Role::Implementer, 0),
        ToolName::ReportStepDone,
        json!({"result": "a"}),
    );
    assert!(run_steps(&chain).is_empty());
    assert_eq!(sim.t("T-1").current, 0);
    let chain = sim.turn_ended(agent("T-1", Role::Implementer, 0), TurnOutcome::EndTurn);
    assert_eq!(
        run_steps(&chain),
        vec![("T-1".into(), Role::Implementer, 1)]
    );
}

#[test]
fn needs_changes_inserts_implement_and_review_then_approves() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({"steps": [{"kind": "implement"}, {"kind": "review"}]}));
    let chain = sim.report("T-1", Role::Implementer, None);
    assert_eq!(run_steps(&chain), vec![("T-1".into(), Role::Reviewer, 1)]);
    assert!(
        prompt_of(&chain)
            .contains("Results of earlier steps:\nstep 0 (implement): result of step 0")
    );

    let chain = sim.report("T-1", Role::Reviewer, Some("needs_changes"));
    let kinds: Vec<StepKind> = sim.t("T-1").steps.iter().map(|s| s.kind).collect();
    use StepKind::*;
    assert_eq!(kinds, vec![Implement, Review, Implement, Review, Done]);
    assert_eq!(sim.t("T-1").review_rounds, 1);
    assert!(has_effect(&chain, |e| matches!(e,
        Effect::StopAgent { agent } if agent.role == Role::Reviewer && agent.step == 1)));
    assert_eq!(
        run_steps(&chain),
        vec![("T-1".into(), Role::Implementer, 2)]
    );
    let prompt = prompt_of(&chain);
    assert!(
        prompt.contains("Address these findings:\nresult of step 1"),
        "{prompt}"
    );
    assert!(prompt.contains("Original instruction:\ndo it"), "{prompt}");

    let chain = sim.report("T-1", Role::Implementer, None);
    assert_eq!(run_steps(&chain), vec![("T-1".into(), Role::Reviewer, 3)]);
    sim.report("T-1", Role::Reviewer, Some("approve"));
    assert_eq!(sim.status("T-1"), TaskStatus::Done);
}

#[test]
fn review_rounds_exhausted_asks_the_orchestrator() {
    let mut sim = Sim::with_config(DomainConfig {
        max_review_rounds: 1,
    });
    sim.group();
    sim.task(json!({"steps": [{"kind": "implement"}, {"kind": "review"}]}));
    sim.report("T-1", Role::Implementer, None);
    sim.report("T-1", Role::Reviewer, Some("needs_changes"));
    sim.report("T-1", Role::Implementer, None);
    let chain = sim.report("T-1", Role::Reviewer, Some("needs_changes"));
    assert_eq!(sim.status("T-1"), TaskStatus::Handling);
    assert_eq!(sim.t("T-1").steps.len(), 5, "no third round inserted");
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::StopAgent { .. }
    )));
    let help = sim.state.open_help("T-1").expect("help").clone();
    assert_eq!(help.kind, HelpKind::ReviewRoundsExhausted);
    assert_eq!(help.source, HelpSource::Yhtye);

    // Resuming accepts the work as is: the task goes on to `done`.
    sim.orch_ok(
        ToolName::AnswerHelp,
        json!({"help_id": help.id, "action": "resume"}),
    );
    assert_eq!(sim.status("T-1"), TaskStatus::Done);
}

#[test]
fn dependency_without_instruction_asks_for_it_when_resolved() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    let chain = sim.task(json!({"depends_on": ["T-1"], "instruction": null}));
    assert!(run_steps(&chain).is_empty());
    assert_eq!(sim.status("T-2"), TaskStatus::Pending);
    sim.deliver_inbox();

    sim.report("T-1", Role::Implementer, None);
    assert_eq!(sim.status("T-2"), TaskStatus::AwaitingInstruction);
    assert_eq!(sim.inbox_kinds(), vec![InboxKind::InstructionNeeded]);
    assert_eq!(
        sim.state.inbox[0].item.render(),
        "[yhtye:instruction_needed] task=T-2\nT-1: result of step 0"
    );
    let (reply, chain) = sim.orch_ok(
        ToolName::SetInstruction,
        json!({"task_id": "T-2", "instruction": "use T-1's result"}),
    );
    assert_eq!(reply["status"], json!("running"));
    assert_eq!(
        run_steps(&chain),
        vec![("T-2".into(), Role::Implementer, 0)]
    );
    assert!(prompt_of(&chain).contains("use T-1's result"));
}

#[test]
fn dependency_with_instruction_starts_automatically() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    sim.task(json!({"depends_on": ["T-1", "T-1"]}));
    assert_eq!(
        sim.t("T-2").depends_on,
        vec!["T-1".to_string()],
        "deduplicated"
    );
    let chain = sim.report("T-1", Role::Implementer, None);
    assert_eq!(
        run_steps(&chain),
        vec![("T-2".into(), Role::Implementer, 0)]
    );
}

#[test]
fn checkpoint_wakes_the_orchestrator_and_continue_passes_the_note() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(
        json!({"steps": [{"kind": "implement"}, {"kind": "checkpoint"}, {"kind": "implement"}]}),
    );
    sim.report("T-1", Role::Implementer, None);
    assert_eq!(sim.status("T-1"), TaskStatus::Checkpoint);
    assert_eq!(
        sim.state.inbox[0].item.render(),
        "[yhtye:checkpoint_reached] task=T-1 step=1\nstep 0 (implement): result of step 0"
    );
    let (reply, chain) = sim.orch_ok(
        ToolName::ResolveCheckpoint,
        json!({"task_id": "T-1", "decision": "continue", "note": "also update docs"}),
    );
    assert_eq!(reply["status"], json!("running"));
    assert_eq!(reply["current_step"], json!(2));
    assert!(prompt_of(&chain).contains("Note from the orchestrator:\nalso update docs"));
}

#[test]
fn checkpoint_abort_cancels_the_task() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({"steps": [{"kind": "checkpoint"}, {"kind": "implement"}]}));
    assert_eq!(sim.status("T-1"), TaskStatus::Checkpoint);
    let (reply, chain) = sim.orch_ok(
        ToolName::ResolveCheckpoint,
        json!({"task_id": "T-1", "decision": "abort"}),
    );
    assert_eq!(reply["status"], json!("cancelled"));
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::StopTaskAgents { .. }
    )));
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::Git(GitOp::RemoveWorkspace { .. })
    )));
}

#[test]
fn agent_help_is_answered_with_a_prompt_to_the_same_session() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    let chain = sim.sub_ok(
        sub("T-1", Role::Implementer, 0),
        ToolName::Help,
        json!({"kind": "question", "message": "which file?"}),
    );
    assert_eq!(
        chain.reply,
        Some(Ok(json!({"help_id": "H-1", "next": "end_turn_and_wait"})))
    );
    assert_eq!(sim.status("T-1"), TaskStatus::Handling);
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::WakeOrchestrator { .. }
    )));
    // The silent turn end after `help` is not a protocol violation.
    let chain = sim.turn_ended(agent("T-1", Role::Implementer, 0), TurnOutcome::EndTurn);
    assert!(chain.effects.is_empty());

    let (reply, chain) = sim.orch_ok(
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "resume", "reply": "README.md"}),
    );
    assert_eq!(reply["task_status"], json!("running"));
    assert!(has_effect(&chain, |e| matches!(e,
        Effect::PromptAgent { agent, text }
            if agent.step == 0 && text == "[yhtye:help_answer] help_id=H-1\nREADME.md")));
    sim.report("T-1", Role::Implementer, None);
    assert_eq!(sim.status("T-1"), TaskStatus::Done);
}

#[test]
fn cancel_task_stops_agents_closes_help_and_settles() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    sim.sub_ok(
        sub("T-1", Role::Implementer, 0),
        ToolName::Help,
        json!({"kind": "blocked", "message": "stuck"}),
    );
    sim.deliver_inbox();
    let (reply, chain) = sim.orch_ok(
        ToolName::CancelTask,
        json!({"task_id": "T-1", "reason": "not needed"}),
    );
    assert_eq!(reply, json!({"task_id": "T-1", "status": "cancelled"}));
    assert_eq!(sim.t("T-1").cancel_reason.as_deref(), Some("not needed"));
    assert_eq!(
        sim.state.help("H-1").map(|h| h.state),
        Some(HelpState::Closed)
    );
    assert!(has_effect(
        &chain,
        |e| matches!(e, Effect::StopTaskAgents { task } if task == "T-1")
    ));
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::Git(GitOp::RemoveWorkspace { .. })
    )));
    assert_eq!(sim.inbox_kinds(), vec![InboxKind::GroupSettled]);
    assert!(sim.state.inbox[0].item.body.contains("T-1 cancelled"));
}

#[test]
fn cancelled_dependency_blocks_dependents_and_settles_the_group() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    sim.task(json!({"depends_on": ["T-1"]}));
    sim.task(json!({"depends_on": ["T-2"]}));
    sim.orch_ok(
        ToolName::CancelTask,
        json!({"task_id": "T-1", "reason": "x"}),
    );
    assert_eq!(sim.status("T-2"), TaskStatus::Pending);
    let body = &sim.state.inbox.last().expect("settled").item.body;
    assert!(body.contains("T-2 cannot start"), "{body}");
    assert!(body.contains("T-3 cannot start"), "{body}");
    let code = sim.orch_err(
        ToolName::FinishGroup,
        json!({"group_id": "G-1", "summary": "s"}),
    );
    assert_eq!(code, ErrorCode::InvalidState);
}

#[test]
fn cancel_group_cancels_open_tasks_without_waking_the_orchestrator() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({}));
    sim.task(json!({"depends_on": ["T-1"]}));
    sim.deliver_inbox();
    let (reply, chain) = sim.orch_ok(
        ToolName::CancelGroup,
        json!({"group_id": "G-1", "reason": "user changed their mind"}),
    );
    assert_eq!(reply["status"], json!("cancelled"));
    assert_eq!(sim.status("T-1"), TaskStatus::Cancelled);
    assert_eq!(sim.status("T-2"), TaskStatus::Cancelled);
    assert!(sim.state.inbox.is_empty());
    assert!(
        !chain
            .effects
            .iter()
            .any(|e| matches!(e, Effect::WakeOrchestrator { .. }))
    );
    assert_eq!(
        sim.state.group("G-1").map(|g| g.status),
        Some(GroupStatus::Cancelled)
    );
}

#[test]
fn modify_steps_replaces_only_unstarted_steps() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(
        json!({"steps": [{"kind": "implement"}, {"kind": "checkpoint"}, {"kind": "implement"}]}),
    );
    sim.report("T-1", Role::Implementer, None);
    let (reply, _) = sim.orch_ok(
        ToolName::ModifySteps,
        json!({"task_id": "T-1", "steps": [{"kind": "review"}, {"kind": "implement", "instruction": "polish"}]}),
    );
    let steps = reply["steps"].as_array().expect("steps");
    let kinds: Vec<&str> = steps.iter().filter_map(|s| s["kind"].as_str()).collect();
    assert_eq!(
        kinds,
        vec!["implement", "checkpoint", "review", "implement", "done"]
    );
    assert_eq!(steps[0]["status"], json!("done"));
    assert_eq!(steps[1]["status"], json!("running"));
    assert_eq!(steps[3]["instruction"], json!("polish"));
    let (_, chain) = sim.orch_ok(
        ToolName::ResolveCheckpoint,
        json!({"task_id": "T-1", "decision": "continue"}),
    );
    assert_eq!(run_steps(&chain), vec![("T-1".into(), Role::Reviewer, 2)]);
}

#[test]
fn modify_steps_with_an_empty_list_leaves_only_done() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({"steps": [{"kind": "checkpoint"}, {"kind": "implement"}]}));
    sim.orch_ok(
        ToolName::ModifySteps,
        json!({"task_id": "T-1", "steps": []}),
    );
    sim.orch_ok(
        ToolName::ResolveCheckpoint,
        json!({"task_id": "T-1", "decision": "continue"}),
    );
    assert_eq!(sim.status("T-1"), TaskStatus::Done);
}

#[test]
fn user_message_is_queued_and_delivered() {
    let mut sim = Sim::new();
    let chain = sim
        .run(DomainCommand::UserMessage {
            chat: "C-1".into(),
            text: "hi".into(),
        })
        .expect("ok");
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::WakeOrchestrator { .. }
    )));
    assert_eq!(
        render_batch(&[sim.state.inbox[0].item.clone()]),
        "[yhtye:user_message]\nhi"
    );
    sim.run(DomainCommand::UserMessage {
        chat: "C-1".into(),
        text: "again".into(),
    })
    .expect("ok");
    assert_eq!(
        sim.state.inbox.iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![1, 2]
    );
    sim.run(DomainCommand::InboxDelivered {
        chat: "C-1".into(),
        up_to: 1,
    })
    .expect("ok");
    assert_eq!(sim.state.inbox.len(), 1);
    sim.deliver_inbox();
    assert!(sim.state.inbox.is_empty());
}

#[test]
fn status_queries_describe_the_group_and_task() {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({"steps": [{"kind": "implement"}, {"kind": "review"}]}));
    let (status, _) = sim.orch_ok(ToolName::GetStatus, json!({}));
    assert_eq!(status["group"]["id"], json!("G-1"));
    assert_eq!(status["tasks"][0]["current_step"], json!("implement"));
    assert_eq!(
        status["tasks"][0]["steps_summary"],
        json!(["implement:running", "review:pending", "done:pending"])
    );
    let (task, _) = sim.orch_ok(ToolName::GetTask, json!({"task_id": "T-1"}));
    assert_eq!(task["branch"], json!("yhtye/G-1-T-1"));
    assert_eq!(task["worktree"], json!("/wt/T-1"));
    assert_eq!(task["steps"][0]["status"], json!("running"));
}
