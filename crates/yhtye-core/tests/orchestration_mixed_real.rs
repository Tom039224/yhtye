//! Real mixed harnesses (Stage 7c-2) through the settings: Claude Code (always
//! Haiku) and OpenCode (always `opencode/muse-spark-1.3-contributor-free`)
//! registered as presets, each role picking one, on a temporary git repository
//! whose group is merged. Ignored by default:
//! `cargo test -p yhtye-core --test orchestration_mixed_real -- --ignored --test-threads=1 --nocapture`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::opencode::{MODEL as FREE, assert_no_new_servers, opencode_servers};
use common::orch::{shutdown_and_check, tool_calls, until};
use common::real::{MODEL as HAIKU, REAL_TIMEOUT, is_orchestrator_turn_end, real_git_config};
use common::repo::TempRepo;
use yhtye_core::acp::{AgentEvent, AgentOutput};
use yhtye_core::agents::{
    AgentCatalog, AgentChoice, AgentRole, HarnessPreset, OPENCODE_FALLBACK_MODEL, RoleSettings,
    inherited_opencode_env_remove,
};
use yhtye_core::api::{ApiEvent, ApiEventBody};
use yhtye_core::domain::DomainEvent;
use yhtye_core::runtime::{ORCHESTRATOR_SESSION, Orchestration};

fn claude() -> AgentChoice {
    AgentChoice::new("claude-code", Some(HAIKU))
}

fn opencode() -> AgentChoice {
    AgentChoice::new("opencode", Some(FREE))
}

/// Both presets as the app registers them (`installed_presets`), Claude Code
/// (Haiku) the built-in default, and `roles` set to `choice` globally.
fn catalog(roles: &[(AgentRole, AgentChoice)]) -> AgentCatalog {
    let presets = vec![
        HarnessPreset::claude_code(HAIKU),
        HarnessPreset::opencode(
            OPENCODE_FALLBACK_MODEL,
            inherited_opencode_env_remove(|k| std::env::var_os(k)),
        ),
    ];
    let agents = AgentCatalog::new(presets, claude());
    for (role, choice) in roles {
        agents.set(None, *role, Some(RoleSettings::only(choice.clone())));
    }
    agents
}

/// The agent recorded by each `session_started`, by session key.
fn started(events: &[ApiEvent]) -> Vec<(String, AgentChoice)> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted {
                session,
                agent: Some(agent),
                replaced,
                ..
            } => {
                assert!(replaced.is_none(), "{session}: nothing is replaced");
                Some((session.clone(), agent.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Every ready session runs the model of the harness it was started with.
fn assert_models(events: &[ApiEvent]) {
    let agents = started(events);
    for e in events {
        if let ApiEventBody::Agent {
            session,
            event: AgentEvent::Ready(info),
        } = &e.body
        {
            let agent = agents
                .iter()
                .rev()
                .find(|(s, _)| s == session)
                .map(|(_, a)| a.clone())
                .unwrap_or_else(|| panic!("{session} was started"));
            let model = info.config_value("model");
            eprintln!(
                "{session}: {} model={model:?} mode={:?}",
                agent.harness,
                info.current_mode()
            );
            assert_eq!(model, agent.model.as_deref(), "{session}");
        }
    }
}

/// The MCP tools an OpenCode session ran as code (`execute` → `rawInput.code`).
fn execute_code(events: &[ApiEvent], session: &str) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::Agent {
                session: s,
                event:
                    AgentEvent::Output(
                        out @ (AgentOutput::ToolCall(_) | AgentOutput::ToolCallUpdate(_)),
                    ),
            } if s == session => {
                let v = serde_json::to_value(out).ok()?;
                v.pointer("/update/rawInput/code")
                    .and_then(|c| c.as_str())
                    .map(str::to_string)
            }
            _ => None,
        })
        .collect()
}

fn group_merged(e: &ApiEvent) -> bool {
    matches!(
        &e.body,
        ApiEventBody::Domain {
            event: DomainEvent::GroupMergeFinished { ok: true, .. }
        }
    )
}

async fn run(
    repo: &TempRepo,
    roles: &[(AgentRole, AgentChoice)],
    request: &str,
) -> (Orchestration, Vec<ApiEvent>) {
    let mut cfg = real_git_config(repo);
    cfg.agents = Arc::new(catalog(roles));
    let (orch, mut rx) = Orchestration::start(cfg)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut events = Vec::new();
    orch.send_user_message(request).expect("send");
    until(&mut rx, &mut events, REAL_TIMEOUT, group_merged).await;
    until(&mut rx, &mut events, REAL_TIMEOUT, is_orchestrator_turn_end).await;
    eprintln!("tool calls: {:#?}", tool_calls(&events));
    (orch, events)
}

/// Claude Code (Haiku) orchestrator, OpenCode (free) implementer, Claude Code
/// (Haiku) reviewer: implement → review, merged into the base branch.
#[tokio::test]
#[ignore = "real Claude Code (Haiku) + OpenCode (muse-spark free)"]
async fn real_mixed_claude_orchestrator_opencode_implementer_claude_reviewer() {
    let servers_before = opencode_servers();
    let r = TempRepo::new();
    let (orch, events) = run(
        &r,
        &[
            (AgentRole::Implementer, opencode()),
            (AgentRole::Reviewer, claude()),
        ],
        "Create one group with exactly one code task (steps: implement, then review) that \
         appends the line `mixed ok` to README.md. When the group settles, call finish_group.",
    )
    .await;
    assert_models(&events);
    let agents = started(&events);
    eprintln!("sessions: {agents:#?}");
    let of = |pred: &dyn Fn(&str) -> bool| -> Vec<AgentChoice> {
        agents
            .iter()
            .filter(|(s, _)| pred(s))
            .map(|(_, a)| a.clone())
            .collect()
    };
    assert!(
        of(&|s| s == ORCHESTRATOR_SESSION)
            .iter()
            .all(|a| *a == claude())
    );
    let implementers = of(&|s| s.ends_with("/implementer"));
    assert!(
        !implementers.is_empty() && implementers.iter().all(|a| *a == opencode()),
        "{implementers:?}"
    );
    let reviewers = of(&|s| s.contains("/review-"));
    assert!(
        !reviewers.is_empty() && reviewers.iter().all(|a| *a == claude()),
        "{reviewers:?}"
    );

    let calls = tool_calls(&events);
    assert!(
        calls
            .iter()
            .any(|(s, t, ok)| s.ends_with("/implementer") && t == "report_step_done" && *ok),
        "the OpenCode implementer reported through MCP: {calls:?}"
    );
    let code = execute_code(&events, "T-1/implementer");
    eprintln!("implementer execute code: {code:#?}");
    assert!(
        code.iter()
            .any(|c| c.contains("tools.yhtye.report_step_done")),
        "OpenCode's `execute` carries the MCP call in rawInput.code"
    );
    assert!(
        r.read("README.md").contains("mixed ok"),
        "merged into the base branch"
    );
    shutdown_and_check(orch, &events).await;
    assert_no_new_servers(&servers_before, Duration::from_secs(10)).await;
}

/// A short run with an OpenCode (free) orchestrator and a Claude Code (Haiku)
/// implementer. The OpenCode orchestrator runs in `build` mode (no read-only
/// restriction, `acp-harnesses.md` §7.3); only the prompt keeps it from writing.
#[tokio::test]
#[ignore = "real Claude Code (Haiku) + OpenCode (muse-spark free)"]
async fn real_opencode_orchestrator_with_a_claude_implementer() {
    let servers_before = opencode_servers();
    let r = TempRepo::new();
    let (orch, events) = run(
        &r,
        &[(AgentRole::Orchestrator, opencode())],
        "Create a group with one code task (steps: implement only) that appends the line \
         `opencode orchestrated` to README.md. When the group settles, call finish_group.",
    )
    .await;
    assert_models(&events);
    let agents = started(&events);
    eprintln!("sessions: {agents:#?}");
    assert!(
        agents
            .iter()
            .any(|(s, a)| s == ORCHESTRATOR_SESSION && *a == opencode())
    );
    assert!(
        agents
            .iter()
            .any(|(s, a)| s.ends_with("/implementer") && *a == claude())
    );
    let calls = tool_calls(&events);
    for tool in ["create_group", "create_task", "finish_group"] {
        assert!(
            calls.contains(&(ORCHESTRATOR_SESSION.into(), tool.into(), true)),
            "{tool}: {calls:?}"
        );
    }
    assert!(r.read("README.md").contains("opencode orchestrated"));
    shutdown_and_check(orch, &events).await;
    assert_no_new_servers(&servers_before, Duration::from_secs(10)).await;
}
