//! Restarting on a stored state: `Restart` marks in-flight work as interrupted,
//! `ResumeTask` continues it (`orchestration-model.md` §10).

use serde_json::json;

use super::*;
use crate::prompts::RESTART_NOTE;

const IDLE: OrchestratorResume = OrchestratorResume {
    had_session: true,
    restored: true,
    turn_was_running: false,
};

fn running(steps: Value) -> Sim {
    let mut sim = Sim::new();
    sim.group();
    sim.task(json!({ "steps": steps }));
    sim.deliver_inbox();
    sim
}

fn restart(sim: &mut Sim, orchestrator: OrchestratorResume) -> Chain {
    sim.run(DomainCommand::Restart {
        orchestrators: vec![("C-1".into(), orchestrator)],
    })
    .expect("restart never fails")
}

fn resume(sim: &mut Sim, task: &str) -> Chain {
    sim.run(DomainCommand::ResumeTask { task: task.into() })
        .expect("resume never fails")
}

fn resume_step(chain: &Chain) -> (AgentRef, String, String) {
    chain
        .effects
        .iter()
        .find_map(|e| match e {
            Effect::ResumeStep {
                agent,
                prompt,
                fallback,
                ..
            } => Some((agent.clone(), prompt.clone(), fallback.clone())),
            _ => None,
        })
        .expect("a ResumeStep effect")
}

#[test]
fn a_running_agent_step_is_interrupted_then_resumed_in_its_session() {
    let mut sim = running(json!([{"kind": "implement"}, {"kind": "review"}]));
    let chain = restart(&mut sim, IDLE);
    assert_eq!(sim.status("T-1"), TaskStatus::Interrupted);
    assert!(chain.effects.is_empty(), "nothing runs before ResumeTask");
    assert!(
        sim.state.inbox.is_empty(),
        "an idle restored orchestrator is not woken"
    );

    let chain = resume(&mut sim, "T-1");
    assert_eq!(sim.status("T-1"), TaskStatus::Running);
    let (agent_ref, prompt, fallback) = resume_step(&chain);
    assert_eq!(agent_ref, agent("T-1", Role::Implementer, 0));
    assert!(
        prompt.starts_with("[yhtye:resume] task=T-1 step=1\n"),
        "{prompt}"
    );
    assert!(fallback.starts_with("[yhtye:step] task=T-1 kind=code step=1/3"));
    assert!(fallback.ends_with(RESTART_NOTE));
    assert_eq!(sim.t("T-1").steps[0].status, StepStatus::Running);

    // The resumed session reports as usual and the task goes on.
    let chain = sim.report("T-1", Role::Implementer, None);
    assert_eq!(run_steps(&chain), vec![("T-1".into(), Role::Reviewer, 1)]);
}

#[test]
fn a_reported_step_whose_turn_end_was_lost_advances() {
    let mut sim = running(json!([{"kind": "implement"}, {"kind": "review"}]));
    sim.sub_ok(
        sub("T-1", Role::Implementer, 0),
        ToolName::ReportStepDone,
        json!({"result": "done before the crash"}),
    );
    restart(&mut sim, IDLE);
    assert_eq!(sim.status("T-1"), TaskStatus::Interrupted);
    let chain = resume(&mut sim, "T-1");
    assert_eq!(run_steps(&chain), vec![("T-1".into(), Role::Reviewer, 1)]);
    assert_eq!(sim.status("T-1"), TaskStatus::Running);
}

#[test]
fn an_interrupted_merge_is_retried() {
    let mut sim = running(json!([{"kind": "implement"}]));
    // As if Yhtye died while git was merging the task branch.
    let t = sim.state.task_mut("T-1").expect("task");
    t.steps[0].status = StepStatus::Done;
    t.steps[1].status = StepStatus::Running;
    t.current = 1;
    t.status = TaskStatus::Merging;
    restart(&mut sim, IDLE);
    assert_eq!(sim.status("T-1"), TaskStatus::Interrupted);
    let chain = resume(&mut sim, "T-1");
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::Git(GitOp::FinishTask { .. })
    )));
    assert_eq!(sim.status("T-1"), TaskStatus::Done);
    assert_eq!(sim.inbox_kinds(), vec![InboxKind::GroupSettled]);
}

#[test]
fn a_task_waiting_for_its_workspace_prepares_it_again() {
    let mut sim = running(json!([{"kind": "implement"}]));
    let t = sim.state.task_mut("T-1").expect("task");
    t.workdir = None;
    t.steps[0].status = StepStatus::Pending;
    restart(&mut sim, IDLE);
    let chain = resume(&mut sim, "T-1");
    assert!(has_effect(&chain, |e| matches!(
        e,
        Effect::Git(GitOp::PrepareWorkspace { .. })
    )));
    assert_eq!(
        run_steps(&chain),
        vec![("T-1".into(), Role::Implementer, 0)]
    );
}

#[test]
fn tasks_waiting_for_the_orchestrator_are_left_alone() {
    let mut sim = running(json!([{"kind": "checkpoint"}, {"kind": "implement"}]));
    sim.task(json!({"instruction": null, "depends_on": []}));
    assert_eq!(sim.status("T-1"), TaskStatus::Checkpoint);
    assert_eq!(sim.status("T-2"), TaskStatus::AwaitingInstruction);
    let inbox = sim.state.inbox.clone();
    let chain = restart(&mut sim, IDLE);
    assert!(chain.events.is_empty(), "{:?}", chain.events);
    assert_eq!(sim.state.inbox, inbox, "undelivered inbox is kept");
    assert!(
        resume(&mut sim, "T-1").events.is_empty(),
        "only interrupted tasks resume"
    );
}

#[test]
fn an_agent_waiting_for_a_help_answer_is_marked_lost() {
    let mut sim = running(json!([{"kind": "implement"}]));
    sim.sub_ok(
        sub("T-1", Role::Implementer, 0),
        ToolName::Help,
        json!({"kind": "question", "message": "which file?"}),
    );
    restart(&mut sim, IDLE);
    assert_eq!(sim.status("T-1"), TaskStatus::Handling);
    assert!(sim.state.help("H-1").expect("help").agent_lost);
    // Resuming restarts the step (the session is restored by the runtime if it
    // can be); the reply becomes a note, and is not required.
    let (_, chain) = sim.orch_ok(
        ToolName::AnswerHelp,
        json!({"help_id": "H-1", "action": "resume", "reply": "README.md"}),
    );
    assert_eq!(
        run_steps(&chain),
        vec![("T-1".into(), Role::Implementer, 0)]
    );
    assert!(prompt_of(&chain).contains("Note from the orchestrator:\nREADME.md"));
}

#[test]
fn an_interrupted_group_merge_is_reported_as_blocked() {
    let mut sim = running(json!([{"kind": "implement"}]));
    sim.report("T-1", Role::Implementer, None);
    sim.deliver_inbox();
    sim.state.group_mut("G-1").expect("group").status = GroupStatus::Finishing;
    restart(&mut sim, IDLE);
    let g = sim.state.group("G-1").expect("group");
    assert_eq!(g.status, GroupStatus::MergeBlocked);
    assert_eq!(sim.inbox_kinds(), vec![InboxKind::MergeResult]);
    assert!(
        sim.state.inbox[0]
            .item
            .render()
            .starts_with("[yhtye:merge_result] group=G-1 ok=false\n")
    );
}

#[test]
fn the_orchestrator_is_told_what_it_missed() {
    let mut sim = running(json!([{"kind": "implement"}]));
    restart(
        &mut sim,
        OrchestratorResume {
            turn_was_running: true,
            ..IDLE
        },
    );
    assert_eq!(sim.inbox_kinds(), vec![InboxKind::Restarted]);
    assert!(sim.state.inbox[0].item.body.contains("cut off"));
    sim.deliver_inbox();

    restart(
        &mut sim,
        OrchestratorResume {
            restored: false,
            ..IDLE
        },
    );
    let body = &sim.state.inbox[0].item.body;
    assert!(body.contains("could not be restored"), "{body}");
    assert!(body.contains("group G-1 [active]: g"), "{body}");
    assert!(
        body.contains("task T-1 [interrupted] step 1/2: t"),
        "{body}"
    );

    sim.deliver_inbox();
    restart(
        &mut sim,
        OrchestratorResume {
            had_session: false,
            restored: false,
            turn_was_running: false,
        },
    );
    assert!(
        sim.state.inbox.is_empty(),
        "a first start has nothing to tell"
    );
}
