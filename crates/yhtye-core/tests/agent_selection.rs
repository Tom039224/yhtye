//! Harness × model selection (Stage 7b, `core-design.md` §15) through the
//! [`Core`] API with fake agents: the settings API and its layers, the model
//! list read over ACP, `create_task` overrides within the candidates (and their
//! rejection), `get_status`'s `agents`, and that changed settings only apply to
//! sessions started afterwards (including sessions restored after a restart).

mod common;

use std::time::Duration;

use common::fake_harness;
use common::repo::TempRepo;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use yhtye_core::acp::{AgentEvent, HarnessConfig};
use yhtye_core::agents::{AgentChoice, AgentRole, HarnessPreset, RoleSettings};
use yhtye_core::api::{
    AgentSettingsView, ApiCommand, ApiErrorCode, ApiEvent, ApiEventBody, ApiResponse, TextKind,
};
use yhtye_core::domain::DomainEvent;
use yhtye_core::runtime::{Core, CoreConfig, ORCHESTRATOR_SESSION};

const TIMEOUT: Duration = Duration::from_secs(30);
const MODELS: [&str; 3] = ["default", "haiku", "sonnet"];

fn with_models(mut script: Value) -> Value {
    script["models"] = json!(MODELS);
    script
}

fn harness(script: Value) -> HarnessConfig {
    fake_harness(with_models(script))
}

/// Two fake harnesses: `fake` (the built-in default, haiku) and `fake-b`,
/// whose implementer says so.
fn config(r: &TempRepo, orch: Value, implementer: Value) -> CoreConfig {
    let reviewer = json!({"turns": []});
    let b_implementer = json!({"turns": [{"match": "[yhtye:step]", "actions": [
        {"message": "from-b"}, "report_state",
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "b done"}}}
    ]}]});
    let mut cfg = CoreConfig::claude_code(&r.data, "haiku");
    cfg.harnesses = vec![
        HarnessPreset::fixed(
            "fake",
            harness(orch.clone()),
            harness(implementer),
            harness(reviewer.clone()),
        ),
        HarnessPreset::fixed(
            "fake-b",
            harness(orch),
            harness(b_implementer),
            harness(reviewer),
        ),
    ];
    cfg.default_agent = AgentChoice::new("fake", Some("haiku"));
    cfg.usage = None;
    cfg
}

async fn run(core: &Core, cmd: ApiCommand) -> ApiResponse {
    core.command(cmd.clone())
        .await
        .unwrap_or_else(|e| panic!("{cmd:?} failed: {e}"))
}

async fn open(core: &Core, r: &TempRepo) -> String {
    let cmd = ApiCommand::OpenProject {
        path: r.repo.display().to_string(),
    };
    match run(core, cmd).await {
        ApiResponse::Project { project } => project.id,
        other => panic!("unexpected {other:?}"),
    }
}

async fn set(
    core: &Core,
    project: Option<&str>,
    role: AgentRole,
    settings: Option<RoleSettings>,
) -> AgentSettingsView {
    let cmd = ApiCommand::SetAgentSettings {
        project: project.map(str::to_string),
        role,
        settings,
    };
    match run(core, cmd).await {
        ApiResponse::AgentSettings { settings } => *settings,
        other => panic!("unexpected {other:?}"),
    }
}

fn c(harness: &str, model: Option<&str>) -> AgentChoice {
    AgentChoice::new(harness, model)
}

fn settings(candidates: &[AgentChoice], default: &AgentChoice) -> RoleSettings {
    RoleSettings {
        candidates: candidates.to_vec(),
        default: default.clone(),
    }
}

async fn until(
    rx: &mut broadcast::Receiver<ApiEvent>,
    seen: &mut Vec<ApiEvent>,
    done: impl Fn(&ApiEvent) -> bool,
) {
    loop {
        let ev = tokio::time::timeout(TIMEOUT, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out after {} events", seen.len()))
            .expect("event stream open");
        let stop = done(&ev);
        seen.push(ev);
        if stop {
            return;
        }
    }
}

fn domain(ev: &ApiEvent, f: impl Fn(&DomainEvent) -> bool) -> bool {
    matches!(&ev.body, ApiEventBody::Domain { event } if f(event))
}

fn turn_ended(ev: &ApiEvent, key: &str) -> bool {
    matches!(&ev.body, ApiEventBody::Agent { session, event: AgentEvent::TurnEnded(_) } if session == key)
}

/// The agents of the `session_started` events of `key`.
fn started(seen: &[ApiEvent], key: &str) -> Vec<(Option<AgentChoice>, bool)> {
    seen.iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted {
                session,
                agent,
                resumed,
                ..
            } if session == key => Some((agent.clone(), *resumed)),
            _ => None,
        })
        .collect()
}

/// Message texts of session `key`.
fn messages(seen: &[ApiEvent], key: &str) -> Vec<String> {
    seen.iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::AgentText {
                session,
                kind: TextKind::Message,
                text,
            } if session == key => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Results of the orchestrator's calls of `tool`, in order.
fn tool_results(seen: &[ApiEvent], tool: &str) -> Vec<Result<Value, String>> {
    seen.iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::ToolCalled { record }
                if record.tool == tool && record.binding.session == ORCHESTRATOR_SESSION =>
            {
                Some(
                    record
                        .result
                        .clone()
                        .map_err(|e| format!("{:?}: {}", e.code, e.message)),
                )
            }
            _ => None,
        })
        .collect()
}

fn reports_model(texts: &[String], model: &str) -> bool {
    texts
        .iter()
        .any(|t| t.contains(&format!(";model={model};")))
}

#[tokio::test]
async fn create_task_picks_agents_within_the_candidates_and_rejects_others() {
    let r = TempRepo::new();
    let orch = json!({"turns": [
        {"match": "[yhtye:user_message]", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "agents"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "sonnet task", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "task-one",
                "model": "sonnet"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "opus task", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "task-two",
                "harness": "fake", "model": "opus"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "b task", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "task-three",
                "harness": "fake-b"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "default task", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "task-four"}}},
            {"mcp_call": {"tool": "get_status", "args": {}}}
        ]},
        {"match": "[yhtye:group_settled]", "actions": [
            {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "ok"}}}
        ]}
    ]});
    let implementer = json!({"turns": [{"match": "[yhtye:step]", "actions": [
        "report_state",
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "done"}}}
    ]}]});
    let core = Core::start(config(&r, orch, implementer))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    let implementers = settings(
        &[
            c("fake", Some("haiku")),
            c("fake", Some("sonnet")),
            c("fake-b", Some("haiku")),
        ],
        &c("fake", Some("haiku")),
    );
    set(&core, None, AgentRole::Implementer, Some(implementers)).await;

    run(
        &core,
        ApiCommand::SendUserMessage {
            project: project.clone(),
            text: "go".into(),
        },
    )
    .await;
    let mut seen = Vec::new();
    until(&mut rx, &mut seen, |e| {
        domain(e, |d| {
            matches!(d, DomainEvent::GroupMergeFinished { ok: true, .. })
        })
    })
    .await;

    let created = tool_results(&seen, "create_task");
    assert_eq!(created.len(), 4);
    assert!(created[0].is_ok() && created[2].is_ok() && created[3].is_ok());
    let rejected = created[1].as_ref().expect_err("opus is not a candidate");
    assert!(
        rejected.starts_with(
            "InvalidArgument: harness: harness=fake model=opus is not an allowed implementer choice"
        ),
        "{rejected}"
    );
    assert!(
        rejected.contains("fake/haiku, fake/sonnet, fake-b/haiku"),
        "{rejected}"
    );

    // T-1: the override; T-2: fake-b (its model filled in from the candidate); T-3: the default.
    assert_eq!(
        started(&seen, "T-1/implementer"),
        [(Some(c("fake", Some("sonnet"))), false)]
    );
    assert!(reports_model(&messages(&seen, "T-1/implementer"), "sonnet"));
    assert_eq!(
        started(&seen, "T-2/implementer"),
        [(Some(c("fake-b", Some("haiku"))), false)]
    );
    assert!(
        messages(&seen, "T-2/implementer")
            .iter()
            .any(|m| m.contains("from-b"))
    );
    assert_eq!(
        started(&seen, "T-3/implementer"),
        [(Some(c("fake", Some("haiku"))), false)]
    );
    assert!(reports_model(&messages(&seen, "T-3/implementer"), "haiku"));
    assert_eq!(
        started(&seen, ORCHESTRATOR_SESSION),
        [(Some(c("fake", Some("haiku"))), false)]
    );

    let status = tool_results(&seen, "get_status").remove(0).expect("status");
    assert_eq!(
        status["agents"]["implementer"]["default"],
        json!({"harness": "fake", "model": "haiku"})
    );
    assert_eq!(
        status["agents"]["implementer"]["allowed"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(
        status["agents"]["reviewer"]["allowed"],
        json!([{"harness": "fake", "model": "haiku"}])
    );

    let ApiResponse::Snapshot { snapshot } = run(&core, ApiCommand::GetSnapshot { project }).await
    else {
        panic!("snapshot");
    };
    let agents: Vec<Option<AgentChoice>> = snapshot
        .state
        .tasks
        .iter()
        .map(|t| t.agent.clone())
        .collect();
    assert_eq!(
        agents,
        [
            Some(c("fake", Some("sonnet"))),
            Some(c("fake-b", Some("haiku"))),
            None
        ]
    );
    let orchestrator = snapshot
        .sessions
        .iter()
        .find(|s| s.session_key == ORCHESTRATOR_SESSION);
    assert_eq!(
        orchestrator.and_then(|s| s.agent.clone()),
        Some(c("fake", Some("haiku")))
    );
    core.shutdown().await;
}

#[tokio::test]
async fn changed_settings_apply_to_sessions_started_afterwards() {
    let r = TempRepo::new();
    let orch = json!({"turns": [
        {"match": "first", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "later"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "long", "kind": "code",
                "steps": [{"kind": "implement"}, {"kind": "checkpoint"}, {"kind": "implement"}],
                "instruction": "long-task"}}}
        ]},
        {"match": "[yhtye:checkpoint_reached]", "actions": [{"message": "waiting"}]},
        {"match": "second", "actions": [
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "G-1", "title": "new", "kind": "investigate",
                "steps": [{"kind": "implement"}], "instruction": "new-task"}}},
            {"mcp_call": {"tool": "resolve_checkpoint", "args": {"task_id": "T-1", "decision": "continue"}}}
        ]},
        {"match": "[yhtye:group_settled]", "actions": [
            {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "ok"}}}
        ]}
    ]});
    let implementer = json!({"turns": [{"match": "[yhtye:step]", "actions": [
        "report_state",
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "done"}}}
    ]}]});
    let core = Core::start(config(&r, orch, implementer))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    run(
        &core,
        ApiCommand::SendUserMessage {
            project: project.clone(),
            text: "first".into(),
        },
    )
    .await;
    let mut seen = Vec::new();
    until(&mut rx, &mut seen, |e| {
        matches!(&e.body, ApiEventBody::Prompted { session, text }
            if session == ORCHESTRATOR_SESSION && text.contains("[yhtye:checkpoint_reached]"))
    })
    .await;
    until(&mut rx, &mut seen, |e| turn_ended(e, ORCHESTRATOR_SESSION)).await;

    // The code task's agent keeps running haiku; new sessions get sonnet.
    let sonnet = RoleSettings::only(c("fake", Some("sonnet")));
    set(
        &core,
        Some(&project),
        AgentRole::Implementer,
        Some(sonnet.clone()),
    )
    .await;
    set(&core, Some(&project), AgentRole::Investigator, Some(sonnet)).await;
    run(
        &core,
        ApiCommand::SendUserMessage {
            project: project.clone(),
            text: "second".into(),
        },
    )
    .await;
    until(&mut rx, &mut seen, |e| {
        domain(e, |d| {
            matches!(d, DomainEvent::GroupMergeFinished { ok: true, .. })
        })
    })
    .await;

    assert_eq!(
        started(&seen, "T-1/implementer"),
        [(Some(c("fake", Some("haiku"))), false)]
    );
    let t1 = messages(&seen, "T-1/implementer");
    assert_eq!(
        t1.iter().filter(|m| m.contains(";model=haiku;")).count(),
        2,
        "{t1:?}"
    );
    assert_eq!(
        started(&seen, "T-2/implementer"),
        [(Some(c("fake", Some("sonnet"))), false)]
    );
    assert!(reports_model(&messages(&seen, "T-2/implementer"), "sonnet"));
    core.shutdown().await;
}

#[tokio::test]
async fn a_restored_session_keeps_the_agent_it_ran() {
    let r = TempRepo::new();
    let orch = json!({"turns": [
        {"match": "[yhtye:user_message]", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "restart"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "slow", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "slow-task"}}}
        ]}
    ]});
    let implementer = json!({"turns": [
        {"match": "slow-task", "actions": [{"message": "working"}, "wait_cancel"]},
        {"match": "[yhtye:resume]", "actions": [
            "report_state",
            {"mcp_call": {"tool": "report_step_done", "args": {"result": "resumed"}}}
        ]}
    ]});
    let cfg = config(&r, orch, implementer);
    let core = Core::start(cfg.clone()).await.expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    set(
        &core,
        None,
        AgentRole::Implementer,
        Some(RoleSettings::only(c("fake", Some("sonnet")))),
    )
    .await;
    run(
        &core,
        ApiCommand::SendUserMessage {
            project: project.clone(),
            text: "go".into(),
        },
    )
    .await;
    let mut seen = Vec::new();
    until(&mut rx, &mut seen, |e| {
        matches!(&e.body, ApiEventBody::Agent { session, event: AgentEvent::Output(_) } if session == "T-1/implementer")
    })
    .await;
    core.shutdown().await;

    // The default is haiku now; the interrupted session still resumes on sonnet.
    let core = Core::start(cfg).await.expect("core again");
    set(
        &core,
        None,
        AgentRole::Implementer,
        Some(RoleSettings::only(c("fake", Some("haiku")))),
    )
    .await;
    let mut rx = core.subscribe();
    open(&core, &r).await;
    let mut seen = Vec::new();
    until(&mut rx, &mut seen, |e| turn_ended(e, "T-1/implementer")).await;
    assert_eq!(
        started(&seen, "T-1/implementer"),
        [(Some(c("fake", Some("sonnet"))), true)]
    );
    assert!(reports_model(&messages(&seen, "T-1/implementer"), "sonnet"));
    core.shutdown().await;
}

#[tokio::test]
async fn settings_are_validated_layered_and_stored() {
    let r = TempRepo::new();
    let idle = json!({"turns": []});
    let cfg = config(&r, idle.clone(), idle);
    let core = Core::start(cfg.clone()).await.expect("core");
    let project = open(&core, &r).await;

    let ApiResponse::AgentSettings { settings: view } =
        run(&core, ApiCommand::GetAgentSettings { project: None }).await
    else {
        panic!("settings");
    };
    let ids: Vec<&str> = view.harnesses.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(ids, ["fake", "fake-b"]);
    assert_eq!(
        view.effective.reviewer,
        RoleSettings::only(c("fake", Some("haiku")))
    );
    assert_eq!((view.project, view.project_layer), (None, None));

    let bad = [
        settings(&[], &c("fake", None)),
        settings(&[c("fake", Some("haiku"))], &c("fake", Some("sonnet"))),
        settings(&[c("nope", None)], &c("nope", None)),
    ];
    for s in bad {
        let cmd = ApiCommand::SetAgentSettings {
            project: None,
            role: AgentRole::Reviewer,
            settings: Some(s),
        };
        assert_eq!(
            core.command(cmd).await.expect_err("invalid").code,
            ApiErrorCode::InvalidArgument
        );
    }
    let cmd = ApiCommand::SetAgentSettings {
        project: Some("nope".into()),
        role: AgentRole::Reviewer,
        settings: None,
    };
    assert_eq!(
        core.command(cmd).await.expect_err("unknown").code,
        ApiErrorCode::NotFound
    );

    let global = settings(
        &[c("fake", Some("haiku")), c("fake-b", None)],
        &c("fake-b", None),
    );
    let local = RoleSettings::only(c("fake", Some("sonnet")));
    set(&core, None, AgentRole::Reviewer, Some(global.clone())).await;
    let view = set(
        &core,
        Some(&project),
        AgentRole::Reviewer,
        Some(local.clone()),
    )
    .await;
    assert_eq!(view.effective.reviewer, local, "the project layer wins");
    assert_eq!(view.global.reviewer, Some(global.clone()));
    core.shutdown().await;

    // Stored: a new core reads the same layers; clearing the project's role inherits.
    let core = Core::start(cfg).await.expect("core again");
    let ApiResponse::AgentSettings { settings: view } = run(
        &core,
        ApiCommand::GetAgentSettings {
            project: Some(project.clone()),
        },
    )
    .await
    else {
        panic!("settings");
    };
    assert_eq!(view.effective.reviewer, local);
    let view = set(&core, Some(&project), AgentRole::Reviewer, None).await;
    assert_eq!(view.effective.reviewer, global);
    assert_eq!(view.project_layer.and_then(|l| l.reviewer), None);
    core.shutdown().await;
}

#[tokio::test]
async fn models_are_read_from_the_harness() {
    let r = TempRepo::new();
    let idle = json!({"turns": []});
    let core = Core::start(config(&r, idle.clone(), idle))
        .await
        .expect("core");
    let cmd = ApiCommand::ListHarnessModels {
        harness: "fake".into(),
        refresh: None,
    };
    let ApiResponse::HarnessModels { models } = run(&core, cmd).await else {
        panic!("models");
    };
    let values: Vec<&str> = models.models.iter().map(|m| m.value.as_str()).collect();
    assert_eq!(values, MODELS);
    assert_eq!(models.harness, "fake");
    assert_eq!(
        models.current.as_deref(),
        Some("default"),
        "the probe does not switch the model"
    );
    let cmd = ApiCommand::ListHarnessModels {
        harness: "nope".into(),
        refresh: Some(true),
    };
    assert_eq!(
        core.command(cmd).await.expect_err("unknown").code,
        ApiErrorCode::NotFound
    );
    core.shutdown().await;
}

#[tokio::test]
async fn a_harness_that_requires_a_model_rejects_a_choice_without_one() {
    let r = TempRepo::new();
    let idle = json!({"turns": []});
    let mut cfg = config(&r, idle.clone(), idle);
    cfg.harnesses[1].requires_model = true; // like OpenCode
    let core = Core::start(cfg).await.expect("core");
    let ApiResponse::AgentSettings { settings: view } =
        run(&core, ApiCommand::GetAgentSettings { project: None }).await
    else {
        panic!("settings");
    };
    let flags: Vec<(&str, bool)> = view
        .harnesses
        .iter()
        .map(|h| (h.id.as_str(), h.requires_model))
        .collect();
    assert_eq!(flags, [("fake", false), ("fake-b", true)]);
    let cmd = ApiCommand::SetAgentSettings {
        project: None,
        role: AgentRole::Implementer,
        settings: Some(settings(
            &[c("fake", Some("haiku")), c("fake-b", None)],
            &c("fake", Some("haiku")),
        )),
    };
    let err = core.command(cmd).await.expect_err("no model");
    assert_eq!(err.code, ApiErrorCode::InvalidArgument);
    assert!(
        err.message.contains("fake-b: a model is required"),
        "{}",
        err.message
    );
    let ok = RoleSettings::only(c("fake-b", Some("sonnet")));
    let view = set(&core, None, AgentRole::Implementer, Some(ok.clone())).await;
    assert_eq!(view.effective.implementer, ok);
    core.shutdown().await;
}

/// A harness that is no longer registered (e.g. OpenCode uninstalled) is
/// replaced by the built-in default when a session starts, and the session
/// says what it replaced (shown in the UI instead of only a log warning).
#[tokio::test]
async fn a_vanished_harness_is_replaced_and_reported() {
    let r = TempRepo::new();
    let orch = json!({"turns": [
        {"match": "[yhtye:user_message]", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "gone"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "t", "kind": "code",
                "steps": [{"kind": "implement"}], "instruction": "go"}}}
        ]},
        {"match": "[yhtye:group_settled]", "actions": [
            {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "ok"}}}
        ]}
    ]});
    let implementer = json!({"turns": [{"match": "[yhtye:step]", "actions": [
        "report_state",
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "done"}}}
    ]}]});
    let cfg = config(&r, orch, implementer);
    let core = Core::start(cfg.clone()).await.expect("core");
    let b = c("fake-b", Some("sonnet"));
    set(
        &core,
        None,
        AgentRole::Implementer,
        Some(RoleSettings::only(b.clone())),
    )
    .await;
    core.shutdown().await;

    // fake-b is not installed any more.
    let mut gone = cfg;
    gone.harnesses.truncate(1);
    let core = Core::start(gone).await.expect("core without fake-b");
    let ApiResponse::AgentSettings { settings: view } =
        run(&core, ApiCommand::GetAgentSettings { project: None }).await
    else {
        panic!("settings");
    };
    assert_eq!(view.harnesses.len(), 1);
    assert_eq!(
        view.effective.implementer.default, b,
        "the stored setting is kept (the UI marks it as unavailable)"
    );
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    run(
        &core,
        ApiCommand::SendUserMessage {
            project,
            text: "go".into(),
        },
    )
    .await;
    let mut seen = Vec::new();
    until(&mut rx, &mut seen, |e| {
        domain(e, |d| {
            matches!(d, DomainEvent::GroupMergeFinished { ok: true, .. })
        })
    })
    .await;
    let replaced: Vec<_> = seen
        .iter()
        .filter_map(|e| match &e.body {
            ApiEventBody::SessionStarted {
                session,
                agent,
                replaced,
                ..
            } => Some((session.clone(), agent.clone(), replaced.clone())),
            _ => None,
        })
        .collect();
    assert!(
        replaced.contains(&(
            "T-1/implementer".into(),
            Some(c("fake", Some("haiku"))),
            Some(b.clone())
        )),
        "{replaced:?}"
    );
    assert!(
        replaced
            .iter()
            .filter(|(s, ..)| s == ORCHESTRATOR_SESSION)
            .all(|(_, _, r)| r.is_none()),
        "{replaced:?}"
    );
    core.shutdown().await;
}
