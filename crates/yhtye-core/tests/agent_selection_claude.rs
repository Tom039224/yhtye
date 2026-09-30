//! Real Claude Code (always Haiku) with harness × model × effort selection
//! (Stage 7b, 7d): the orchestrator sees the candidate rows and their notes, a
//! choice that does not exactly match a row is rejected by `create_task`, an
//! exact one is recorded on the task and its agent session runs the resolved
//! model. Only Haiku is cheap, so the one row is `claude-code/haiku` (no
//! effort: Haiku has none) and the rejected choice asks Haiku for `effort=high`
//! (never started). That an effort really reaches the adapter is checked
//! without a prompt in `acp_claude_real::real_effort_is_applied_after_the_model`.
//! Ignored by default:
//! `cargo test -p yhtye-core --test agent_selection_claude -- --ignored --test-threads=1 --nocapture`.

mod common;

use common::orch::ORCHESTRATOR_SESSION;

use std::sync::Arc;

use common::orch::{shutdown_and_check, tool_calls, until};
use common::real::{MODEL, REAL_TIMEOUT, assert_haiku, is_orchestrator_turn_end, real_git_config};
use common::repo::TempRepo;
use yhtye_core::agents::{
    AgentCatalog, AgentChoice, AgentRole, Candidate, HarnessPreset, RoleSettings,
};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::runtime::Orchestration;

const REQUEST: &str = "Create one group with exactly one code task that appends the line \
'agent ok' to README.md (steps: implement only). I want that task to run on a specific model: \
call create_task with harness=claude-code, model=haiku and effort=high. If Yhtye rejects that choice, \
call create_task again with exactly one of the candidate rows it lists (no effort if the row has \
none). When the group settles, call finish_group.";

fn finish_called(e: &ApiEvent) -> bool {
    matches!(&e.body, ApiEventBody::ToolCalled { record }
        if record.binding.session == ORCHESTRATOR_SESSION && record.tool == "finish_group")
}

#[tokio::test]
#[ignore = "real Claude Code (Haiku)"]
async fn real_orchestrator_override_is_checked_and_runs_the_resolved_model() {
    let r = TempRepo::new();
    let haiku = AgentChoice::new("claude-code", Some(MODEL));
    let agents = AgentCatalog::new(vec![HarnessPreset::claude_code(MODEL)], haiku.clone());
    agents.set(
        None,
        AgentRole::Implementer,
        Some(RoleSettings {
            candidates: vec![Candidate::from(haiku.clone()).with_note("cheap, for small edits")],
            default: haiku.clone(),
        }),
    );
    let mut cfg = real_git_config(&r);
    cfg.agents = Arc::new(agents);
    let (orch, mut rx) = Orchestration::start(cfg)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut events = Vec::new();
    common::orch::send(&orch, REQUEST).await;
    until(&mut rx, &mut events, REAL_TIMEOUT, finish_called).await;
    until(&mut rx, &mut events, REAL_TIMEOUT, is_orchestrator_turn_end).await;
    eprintln!("tool calls: {:#?}", tool_calls(&events));
    assert_haiku(&events);

    let creates: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::ToolCalled { record } if record.tool == "create_task" => Some(record),
            _ => None,
        })
        .collect();
    let rejected = creates
        .iter()
        .find(|r| r.args.get("effort").and_then(|m| m.as_str()) == Some("high"))
        .expect("the orchestrator asked for effort=high");
    let error = rejected
        .result
        .as_ref()
        .expect_err("no row has effort=high");
    eprintln!("rejection: {}", error.message);
    assert!(
        error
            .message
            .contains("candidate rows: harness=claude-code model=haiku (cheap, for small edits)"),
        "{}",
        error.message
    );
    assert!(
        creates.iter().any(|r| r.result.is_ok()),
        "a task was created"
    );

    let snap = orch.snapshot().await.expect("snapshot");
    let task = snap.state.tasks.first().expect("a task");
    eprintln!("task agent: {:?}", task.agent);
    let t_key = format!("{}/implementer", task.id);
    let started: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted { session, agent, .. } if *session == t_key => {
                agent.clone()
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        started,
        std::slice::from_ref(&haiku),
        "the task's session runs the resolved choice"
    );
    let orchestrator = snap
        .sessions
        .iter()
        .find(|s| s.session_key == ORCHESTRATOR_SESSION);
    assert_eq!(orchestrator.and_then(|s| s.agent.clone()), Some(haiku));
    assert!(r.read("README.md").contains("agent ok"), "the work landed");
    shutdown_and_check(orch, &events).await;
}
