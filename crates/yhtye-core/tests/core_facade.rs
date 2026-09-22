//! The [`Core`] API facade with fake agents on a temporary git repository:
//! project registration and validation, commands and their errors, history
//! paging with `ListEvents`, and user cancellation of turns, tasks and groups.
//!
//! `YHTYE_RECORD_FIXTURE=1` also writes the event streams of the run-to-merge
//! and the cancellation tests to `src/test/fixtures/fake-{run,cancel}.json`
//! (input of the frontend's Vitest suite; `pnpm record:fixtures`).

mod common;

use std::path::Path;
use std::time::Duration;

use common::fake_harness;
use common::repo::TempRepo;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use yhtye_core::acp::HarnessConfig;
use yhtye_core::api::{
    ApiCommand, ApiErrorCode, ApiEvent, ApiEventBody, ApiResponse, LoggedEvent, ProjectInfo,
    Snapshot,
};
use yhtye_core::domain::{DomainEvent, GroupStatus, TaskStatus};
use yhtye_core::runtime::{Core, CoreConfig, ORCHESTRATOR_SESSION, USER_SESSION};

const TIMEOUT: Duration = Duration::from_secs(30);

fn core_config(r: &TempRepo, orch: Value, implementer: Value, reviewer: Value) -> CoreConfig {
    let mut cfg = CoreConfig::claude_code(&r.data, "haiku");
    cfg.orchestrator = fake_harness(orch);
    cfg.implementer = fake_harness(implementer);
    cfg.reviewer = fake_harness(reviewer);
    cfg
}

async fn run(core: &Core, cmd: ApiCommand) -> ApiResponse {
    core.command(cmd.clone())
        .await
        .unwrap_or_else(|e| panic!("{cmd:?} failed: {e}"))
}

async fn error_code(core: &Core, cmd: ApiCommand) -> ApiErrorCode {
    match core.command(cmd.clone()).await {
        Ok(r) => panic!("{cmd:?} succeeded: {r:?}"),
        Err(e) => e.code,
    }
}

async fn open(core: &Core, path: &Path) -> ProjectInfo {
    let cmd = ApiCommand::OpenProject {
        path: path.display().to_string(),
    };
    match run(core, cmd).await {
        ApiResponse::Project { project } => project,
        other => panic!("unexpected {other:?}"),
    }
}

async fn snapshot(core: &Core, project: &str) -> Snapshot {
    let cmd = ApiCommand::GetSnapshot {
        project: project.into(),
    };
    match run(core, cmd).await {
        ApiResponse::Snapshot { snapshot } => snapshot,
        other => panic!("unexpected {other:?}"),
    }
}

/// Every stored event, fetched in pages of `page`.
async fn all_events(core: &Core, project: &str, page: u32) -> Vec<LoggedEvent> {
    let mut out: Vec<LoggedEvent> = Vec::new();
    loop {
        let cmd = ApiCommand::ListEvents {
            project: project.into(),
            after_seq: out.last().map_or(0, |e| e.seq),
            limit: Some(page),
        };
        let ApiResponse::Events { events, more } = run(core, cmd).await else {
            panic!("expected events");
        };
        out.extend(events);
        if !more {
            return out;
        }
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
            .unwrap_or_else(|_| {
                panic!(
                    "timed out after {} events: {:#?}",
                    seen.len(),
                    seen.iter()
                        .filter(|e| !e.live)
                        .map(|e| serde_json::to_string(&e.body)
                            .unwrap_or_default()
                            .chars()
                            .take(200)
                            .collect::<String>())
                        .collect::<Vec<_>>()
                )
            })
            .expect("event stream open");
        let stop = done(&ev);
        seen.push(ev);
        if stop {
            return;
        }
    }
}

fn is_domain(ev: &ApiEvent, f: impl Fn(&DomainEvent) -> bool) -> bool {
    matches!(&ev.body, ApiEventBody::Domain { event } if f(event))
}

fn group_done(ev: &ApiEvent) -> bool {
    is_domain(ev, |e| {
        matches!(e, DomainEvent::GroupMergeFinished { ok: true, .. })
    })
}

fn orchestrator_turn_ended(ev: &ApiEvent) -> bool {
    matches!(&ev.body, ApiEventBody::Agent { session, event: yhtye_core::acp::AgentEvent::TurnEnded(_) } if session == ORCHESTRATOR_SESSION)
}

fn full_run_scripts() -> (Value, Value, Value) {
    let orch = json!({"turns": [
        {"match": "[yhtye:user_message]", "actions": [
            {"thought": "The user wants a greeting file. "},
            {"thought": "One code task with a review."},
            {"message": "I will create "},
            {"message": "a group for this."},
            {"tool_call": {"id": "read-1", "title": "Read README.md"}},
            {"tool_call_update": {"id": "read-1", "status": "completed"}},
            {"mcp_call": {"tool": "create_group", "args": {"title": "greeting"}}},
            {"mcp_call": {"tool": "create_task", "args": {
                "group_id": "${group_id}", "title": "add hello.txt", "kind": "code",
                "steps": [{"kind": "implement"}, {"kind": "review"}],
                "instruction": "write-hello"}}},
            {"message": "Task T-1 is running."}
        ]},
        {"match": "[yhtye:group_settled]", "actions": [
            {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "hello.txt added"}}},
            {"message": "Done: hello.txt is on main."}
        ]}
    ]});
    let implementer = json!({"turns": [{"match": "write-hello", "actions": [
        {"message": "Writing the file."},
        {"tool_call": {"id": "w-1", "title": "Write hello.txt"}},
        {"write_file": {"path": "hello.txt", "text": "hello\n"}},
        {"tool_call_update": {"id": "w-1", "status": "completed"}},
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "wrote hello.txt"}}}
    ]}]});
    let reviewer = json!({"turns": [{"match": "[yhtye:step]", "actions": [
        {"message": "Looks good."},
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "fine", "verdict": "approve"}}}
    ]}]});
    (orch, implementer, reviewer)
}

#[tokio::test]
async fn projects_are_validated_registered_and_reopened() {
    let r = TempRepo::new();
    let idle = json!({"turns": []});
    let core = Core::start(core_config(&r, idle.clone(), idle.clone(), idle))
        .await
        .expect("core");
    let ApiResponse::Projects { projects } = run(&core, ApiCommand::ListProjects).await else {
        panic!("expected projects");
    };
    assert!(projects.is_empty());

    let missing = r.data.join("nope").display().to_string();
    let not_top = r.repo.join("sub");
    std::fs::create_dir_all(&not_top).expect("mkdir");
    for path in [missing, String::new(), not_top.display().to_string()] {
        let cmd = ApiCommand::OpenProject { path };
        assert_eq!(error_code(&core, cmd).await, ApiErrorCode::InvalidArgument);
    }

    let p = open(&core, &r.repo).await;
    assert_eq!(
        (p.id.as_str(), p.name.as_str(), p.open),
        ("repo", "repo", true)
    );
    assert_eq!(open(&core, &r.repo).await, p, "opening again is a no-op");
    let ApiResponse::Projects { projects } = run(&core, ApiCommand::ListProjects).await else {
        panic!("expected projects");
    };
    assert_eq!(projects, vec![p.clone()]);

    let unknown = ApiCommand::GetSnapshot {
        project: "other".into(),
    };
    assert_eq!(error_code(&core, unknown).await, ApiErrorCode::NotFound);
    let unknown = ApiCommand::ListEvents {
        project: "other".into(),
        after_seq: 0,
        limit: None,
    };
    assert_eq!(error_code(&core, unknown).await, ApiErrorCode::NotFound);
    let empty = ApiCommand::SendUserMessage {
        project: p.id.clone(),
        text: "  \n".into(),
    };
    assert_eq!(
        error_code(&core, empty).await,
        ApiErrorCode::InvalidArgument
    );

    // A new Core on the same data directory knows the project but has not opened it.
    core.shutdown().await;
    let idle = json!({"turns": []});
    let core = Core::start(core_config(&r, idle.clone(), idle.clone(), idle))
        .await
        .expect("core");
    let ApiResponse::Projects { projects } = run(&core, ApiCommand::ListProjects).await else {
        panic!("expected projects");
    };
    assert_eq!(projects.len(), 1);
    assert!(!projects[0].open);
    let history = all_events(&core, "repo", 100).await;
    assert!(!history.is_empty(), "history is readable while closed");
    assert_eq!(
        open(&core, &r.repo).await.id,
        "repo",
        "same id after a restart"
    );
    core.shutdown().await;
}

#[tokio::test]
async fn a_request_runs_to_the_merge_through_the_facade() {
    let r = TempRepo::new();
    let (orch, implementer, reviewer) = full_run_scripts();
    let core = Core::start(core_config(&r, orch, implementer, reviewer))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r.repo).await.id;
    let start = snapshot(&core, &project).await;
    let send = ApiCommand::SendUserMessage {
        project: project.clone(),
        text: "Add hello.txt please".into(),
    };
    assert!(matches!(run(&core, send).await, ApiResponse::Accepted));
    let mut seen = Vec::new();
    until(&mut rx, &mut seen, group_done).await;
    until(&mut rx, &mut seen, orchestrator_turn_ended).await;

    assert_eq!(r.read("hello.txt"), "hello\n");
    let snap = snapshot(&core, &project).await;
    assert_eq!(snap.state.groups[0].status, GroupStatus::Done);
    assert_eq!(snap.state.tasks[0].status, TaskStatus::Done);
    let orch_session = snap
        .sessions
        .iter()
        .find(|s| s.session_key == ORCHESTRATOR_SESSION)
        .expect("orchestrator session");
    assert!(!orch_session.turn_running, "{orch_session:?}");

    // Paging returns every durable event once, in order, matching the stream.
    let history = all_events(&core, &project, 7).await;
    let seqs: Vec<u64> = history.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (1..=snap.seq).collect::<Vec<_>>());
    let streamed: Vec<u64> = seen.iter().filter(|e| !e.live).map(|e| e.seq).collect();
    assert_eq!(streamed.last(), Some(&snap.seq));
    assert!(seen.iter().any(|e| e.live), "chunks are streamed live");
    assert!(history.iter().any(|e| e.body["type"] == "agent_text"));

    record_fixture("fake-run.json", &start, &seen, &snap);
    core.shutdown().await;
}

/// With `YHTYE_RECORD_FIXTURE` set, writes the stream for the frontend tests
/// (temporary paths and pids in it are just data).
fn record_fixture(name: &str, start: &Snapshot, events: &[ApiEvent], end: &Snapshot) {
    if std::env::var_os("YHTYE_RECORD_FIXTURE").is_none() {
        return;
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/test/fixtures")
        .join(name);
    std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir fixtures");
    let doc = json!({
        "note": "Recorded by crates/yhtye-core/tests/core_facade.rs (YHTYE_RECORD_FIXTURE=1). Test input only.",
        "start": start,
        "events": events,
        "end": end,
    });
    let text = serde_json::to_string_pretty(&doc).expect("json");
    std::fs::write(&path, text + "\n").expect("write fixture");
}

#[tokio::test]
async fn the_user_can_cancel_the_orchestrator_turn_a_task_and_a_group() {
    let r = TempRepo::new();
    let orch = json!({"turns": [
        {"match": "hold on", "actions": [{"message": "thinking..."}, "wait_cancel"]},
        {"match": "start two", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "two"}}},
            {"mcp_call": {"tool": "create_task", "args": {"group_id": "${group_id}", "title": "a",
                "kind": "code", "steps": [{"kind": "implement"}], "instruction": "wait-a"}}},
            {"mcp_call": {"tool": "create_task", "args": {"group_id": "${group_id}", "title": "b",
                "kind": "code", "steps": [{"kind": "implement"}], "instruction": "wait-b"}}}
        ]},
        {"actions": [{"message": "noted"}]}
    ]});
    let implementer = json!({"turns": [{"actions": ["wait_cancel"]}]});
    let core = Core::start(core_config(&r, orch, implementer, json!({"turns": []})))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r.repo).await.id;
    let start = snapshot(&core, &project).await;
    let send = |text: &str| ApiCommand::SendUserMessage {
        project: project.clone(),
        text: text.into(),
    };
    let mut seen = Vec::new();

    run(&core, send("hold on")).await;
    until(&mut rx, &mut seen, |e| {
        matches!(&e.body, ApiEventBody::AgentText { text, .. } if text.contains("thinking"))
            || e.live
    })
    .await;
    let cancel = ApiCommand::CancelOrchestratorTurn {
        project: project.clone(),
    };
    run(&core, cancel).await;
    until(&mut rx, &mut seen, orchestrator_turn_ended).await;

    run(&core, send("start two")).await;
    let running = |task: &'static str| {
        move |e: &ApiEvent| {
            is_domain(
                e,
                |d| matches!(d, DomainEvent::StepStarted { task: t, .. } if t == task),
            )
        }
    };
    until(&mut rx, &mut seen, running("T-2")).await;

    let cancel_task = |task: &str| ApiCommand::CancelTask {
        project: project.clone(),
        task: task.into(),
        reason: None,
    };
    assert_eq!(
        error_code(&core, cancel_task("T-9")).await,
        ApiErrorCode::NotFound
    );
    run(&core, cancel_task("T-1")).await;
    let snap = snapshot(&core, &project).await;
    assert_eq!(snap.state.tasks[0].status, TaskStatus::Cancelled);
    assert_eq!(
        snap.state.tasks[0].cancel_reason.as_deref(),
        Some("cancelled by the user")
    );
    until(&mut rx, &mut seen, |e| {
        matches!(&e.body, ApiEventBody::Prompted { session, text }
            if session == ORCHESTRATOR_SESSION && text.contains("the user cancelled task T-1"))
    })
    .await;
    assert!(seen.iter().any(|e| matches!(&e.body,
        ApiEventBody::ToolCalled { record } if record.binding.session == USER_SESSION
            && record.tool == "cancel_task" && record.result.is_ok())));

    let cancel_group = ApiCommand::CancelGroup {
        project: project.clone(),
        group: "G-1".into(),
        reason: Some("changed my mind".into()),
    };
    run(&core, cancel_group.clone()).await;
    let snap = snapshot(&core, &project).await;
    assert_eq!(snap.state.groups[0].status, GroupStatus::Cancelled);
    assert_eq!(snap.state.tasks[1].status, TaskStatus::Cancelled);
    assert!(
        r.extra_worktrees().is_empty(),
        "the task and integration worktrees are gone: {:?}",
        r.extra_worktrees()
    );
    assert!(r.branch_exists("yhtye/G-1"), "the group branch is kept");
    assert_eq!(
        error_code(&core, cancel_group).await,
        ApiErrorCode::InvalidState
    );
    until(&mut rx, &mut seen, |e| {
        matches!(&e.body, ApiEventBody::Prompted { session, text }
            if session == ORCHESTRATOR_SESSION && text.contains("cancelled group G-1"))
    })
    .await;
    until(&mut rx, &mut seen, orchestrator_turn_ended).await;
    let end = snapshot(&core, &project).await;
    record_fixture("fake-cancel.json", &start, &seen, &end);
    core.shutdown().await;
    let after = ApiCommand::GetSnapshot { project };
    assert_eq!(error_code(&core, after).await, ApiErrorCode::NotFound);
}

#[tokio::test]
async fn projects_with_unfinished_work_are_reopened_and_resumed_on_start() {
    let r = TempRepo::new();
    let idle_repo = TempRepo::new();
    let orch = json!({"turns": [
        {"match": "start one", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "one"}}},
            {"mcp_call": {"tool": "create_task", "args": {"group_id": "${group_id}", "title": "a",
                "kind": "code", "steps": [{"kind": "implement"}], "instruction": "write-a"}}}
        ]},
        {"actions": [{"message": "noted"}]}
    ]});
    // Before the restart the implementer never finishes; afterwards (restored
    // or started again with the step's prompt) it reports.
    let waiting = json!({"turns": [{"actions": [
        {"write_file": {"path": "a.txt", "text": "a\n"}}, "wait_cancel"]}]});
    let reporting = json!({"turns": [{"actions": [
        {"mcp_call": {"tool": "report_step_done", "args": {"result": "wrote a.txt"}}}]}]});
    let cfg = core_config(&r, orch.clone(), waiting, json!({"turns": []}));
    let core = Core::start(cfg).await.expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r.repo).await.id;
    let idle = open(&core, &idle_repo.repo).await.id;
    let send = ApiCommand::SendUserMessage {
        project: project.clone(),
        text: "start one".into(),
    };
    run(&core, send).await;
    let mut seen = Vec::new();
    until(&mut rx, &mut seen, |e| {
        is_domain(
            e,
            |d| matches!(d, DomainEvent::StepStarted { task, .. } if task == "T-1"),
        )
    })
    .await;
    core.shutdown().await;

    let cfg = core_config(&r, orch, reporting, json!({"turns": []}));
    let core = Core::start(cfg).await.expect("core");
    let mut rx = core.subscribe();
    let resumed = core.resume_unfinished().await;
    let ids: Vec<&str> = resumed.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, vec![project.as_str()], "{idle} has no unfinished work");
    assert!(resumed[0].1.as_ref().is_ok_and(|p| p.open));
    let mut seen = Vec::new();
    until(&mut rx, &mut seen, |e| {
        is_domain(e, |d| matches!(d, DomainEvent::TaskStatusChanged { task, status: TaskStatus::Done } if task == "T-1"))
    })
    .await;
    let ApiResponse::Projects { projects } = run(&core, ApiCommand::ListProjects).await else {
        panic!("expected projects");
    };
    let open_ids: Vec<&str> = projects
        .iter()
        .filter(|p| p.open)
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(open_ids, vec![project.as_str()]);
    core.shutdown().await;
}

#[test]
fn claude_code_config_uses_the_given_model_for_every_role() {
    let cfg = CoreConfig::claude_code("/tmp/x", "haiku");
    let model = |h: &HarnessConfig| h.model.as_ref().map(|m| m.value.clone());
    assert_eq!(model(&cfg.orchestrator).as_deref(), Some("haiku"));
    assert_eq!(model(&cfg.implementer).as_deref(), Some("haiku"));
    assert_eq!(model(&cfg.reviewer).as_deref(), Some("haiku"));
}
