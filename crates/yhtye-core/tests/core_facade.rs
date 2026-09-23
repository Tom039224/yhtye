//! The [`Core`] API facade with fake agents on a temporary git repository:
//! project registration and validation, commands and their errors, history
//! paging with `ListEvents`, and user cancellation of turns, tasks and groups.
//!
//! `YHTYE_RECORD_FIXTURE=1` also writes the event streams of the run-to-merge,
//! the cancellation and the unfinished-group tests to
//! `src/test/fixtures/fake-{run,cancel,unfinished}.json`
//! (input of the frontend's Vitest suite; `pnpm record:fixtures`).

mod common;

use std::path::Path;
use std::time::Duration;

use common::fake_harness;
use common::repo::TempRepo;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use yhtye_core::acp::HarnessConfig;
use yhtye_core::agents::{AgentChoice, HarnessPreset};
use yhtye_core::api::{
    ApiCommand, ApiErrorCode, ApiEvent, ApiEventBody, ApiResponse, LoggedEvent, ProjectInfo,
    Snapshot,
};
use yhtye_core::domain::{DomainEvent, GroupStatus, TaskStatus};
use yhtye_core::git::GitOverview;
use yhtye_core::runtime::{Core, CoreConfig, ORCHESTRATOR_SESSION, USER_SESSION};

const TIMEOUT: Duration = Duration::from_secs(30);

fn core_config(r: &TempRepo, orch: Value, implementer: Value, reviewer: Value) -> CoreConfig {
    let mut cfg = CoreConfig::claude_code(&r.data, "haiku");
    cfg.harnesses = vec![HarnessPreset::fixed(
        "fake",
        fake_harness(orch),
        fake_harness(implementer),
        fake_harness(reviewer),
    )];
    cfg.default_agent = AgentChoice::new("fake", Some("haiku"));
    cfg.usage = None;
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

/// The orchestrator reports to the user after `group_settled` but never calls
/// `finish_group` (seen with real Haiku, Stage 7a). With `finish_on_reminder`
/// it calls it when reminded; otherwise Yhtye finishes the group itself.
fn forgetful_scripts(finish_on_reminder: bool) -> (Value, Value, Value) {
    let (orch, implementer, reviewer) = full_run_scripts();
    let mut turns = vec![orch["turns"][0].clone()];
    if finish_on_reminder {
        turns.push(json!({"match": "reminder=1", "actions": [
            {"mcp_call": {"tool": "finish_group", "args": {"group_id": "${group}", "summary": "hello.txt added"}}},
            {"message": "Merged."}
        ]}));
    }
    turns.push(json!({"match": "[yhtye:group_settled]", "actions": [
        {"message": "The test finished: hello.txt was written."}
    ]}));
    turns.push(json!({"match": "[yhtye:merge_result]", "actions": [
        {"message": "Yhtye merged the group into main."}
    ]}));
    (json!({"turns": turns}), implementer, reviewer)
}

async fn run_forgetful(finish_on_reminder: bool) -> (TempRepo, Snapshot, Vec<ApiEvent>, Snapshot) {
    let r = TempRepo::new();
    let (orch, implementer, reviewer) = forgetful_scripts(finish_on_reminder);
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
    let end = snapshot(&core, &project).await;
    core.shutdown().await;
    (r, start, seen, end)
}

fn reminded(ev: &ApiEvent) -> bool {
    is_domain(ev, |e| matches!(e, DomainEvent::GroupFinishReminded { .. }))
}

#[tokio::test]
async fn a_group_the_orchestrator_leaves_open_is_finished_by_yhtye() {
    let (r, start, seen, end) = run_forgetful(false).await;
    assert_eq!(r.read("hello.txt"), "hello\n", "merged into main");
    let g = &end.state.groups[0];
    assert_eq!(g.status, GroupStatus::Done);
    assert_eq!(g.finish_nudges, 1);
    assert_eq!(
        g.finish_summary.as_deref(),
        Some(yhtye_core::prompts::AUTO_FINISH_SUMMARY)
    );
    assert_eq!(seen.iter().filter(|e| reminded(e)).count(), 1);
    assert!(end.state.inbox.is_empty(), "merge_result was delivered");
    record_fixture("fake-unfinished.json", &start, &seen, &end);
}

#[tokio::test]
async fn a_reminded_orchestrator_finishes_the_group_itself() {
    let (r, _, seen, end) = run_forgetful(true).await;
    assert_eq!(r.read("hello.txt"), "hello\n");
    let g = &end.state.groups[0];
    assert_eq!(g.status, GroupStatus::Done);
    assert_eq!(g.finish_summary.as_deref(), Some("hello.txt added"));
    assert_eq!(seen.iter().filter(|e| reminded(e)).count(), 1);
    let finished_by_tool = seen.iter().any(
        |e| matches!(&e.body, ApiEventBody::ToolCalled { record } if record.tool == "finish_group"),
    );
    assert!(finished_by_tool);
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
    assert_eq!(
        cfg.default_agent,
        AgentChoice::new("claude-code", Some("haiku"))
    );
    let preset = &cfg.harnesses[0];
    let model = |h: &HarnessConfig| h.model.as_ref().map(|m| m.value.clone());
    for h in [
        &preset.orchestrator,
        &preset.implementer,
        &preset.investigator,
        &preset.reviewer,
    ] {
        assert_eq!(model(h).as_deref(), Some("haiku"));
    }
}

async fn git_overview(core: &Core, project: &str, limit: Option<u32>) -> GitOverview {
    let cmd = ApiCommand::GetGitOverview {
        project: project.into(),
        limit,
    };
    match run(core, cmd).await {
        ApiResponse::GitOverview { git } => git,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn the_git_overview_shows_branches_and_the_commit_graph() {
    let r = TempRepo::new();
    r.git(&["checkout", "-q", "-b", "yhtye/G-1"]);
    r.write("a.txt", "a\n");
    r.commit("feat: a");
    r.git(&["checkout", "-q", "main"]);
    r.write("b.txt", "b\n");
    r.commit("chore: b");
    r.git(&["merge", "-q", "--no-ff", "-m", "merge G-1", "yhtye/G-1"]);
    let idle = json!({"turns": []});
    let core = Core::start(core_config(&r, idle.clone(), idle.clone(), idle))
        .await
        .expect("core");
    let p = open(&core, &r.repo).await;

    let git = git_overview(&core, &p.id, None).await;
    assert_eq!(git.head.as_deref(), Some("main"));
    let names: Vec<&str> = git.branches.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["main", "yhtye/G-1"]);
    let subjects: Vec<&str> = git.commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects[0], "merge G-1");
    assert_eq!(subjects.len(), 4, "{subjects:?}");
    assert!(!git.truncated);
    let merge = &git.commits[0];
    assert_eq!(Some(&merge.sha), git.head_sha.as_ref());
    assert_eq!(merge.parents.len(), 2, "a merge has two parents");
    assert_eq!(merge.branches, ["main"]);
    let side = git
        .commits
        .iter()
        .find(|c| c.subject == "feat: a")
        .expect("side commit");
    assert_eq!(side.branches, ["yhtye/G-1"]);
    assert_eq!(
        merge.parents[1], side.sha,
        "second parent is the merged branch"
    );
    // Children come before their parents.
    let at = |sha: &str| git.commits.iter().position(|c| c.sha == sha);
    for (i, c) in git.commits.iter().enumerate() {
        for parent in &c.parents {
            assert!(at(parent).is_none_or(|p| p > i), "topological order");
        }
    }

    let short = git_overview(&core, &p.id, Some(2)).await;
    assert_eq!(short.commits.len(), 2);
    assert!(short.truncated);

    // Readable while the project is closed; unknown projects are not found.
    core.shutdown().await;
    let idle = json!({"turns": []});
    let core = Core::start(core_config(&r, idle.clone(), idle.clone(), idle))
        .await
        .expect("core");
    assert_eq!(git_overview(&core, &p.id, None).await.commits.len(), 4);
    let unknown = ApiCommand::GetGitOverview {
        project: "other".into(),
        limit: None,
    };
    assert_eq!(error_code(&core, unknown).await, ApiErrorCode::NotFound);
    core.shutdown().await;
}

/// `/usage` output of claude-agent-acp 0.81 (reset times in UTC, as the probe asks).
const USAGE_MARKDOWN: &str = "## Usage\n\n> Claude max subscription usage\n\n### Limits\n\n\
**5-hour limit** — **77%** · Resets Sep 22, 10:50 PM UTC\n\n`███████████████░░░░░`\n\n\
**Weekly · all models** — **40%** · Resets Sep 24, 5:00 PM UTC\n\n`████████░░░░░░░░░░░░`\n";

async fn usage(core: &Core, refresh: bool) -> yhtye_core::usage::UsageReport {
    match run(
        core,
        ApiCommand::GetUsage {
            refresh: Some(refresh),
        },
    )
    .await
    {
        ApiResponse::Usage { usage } => usage,
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn usage_is_read_from_the_harness_usage_command_and_cached() {
    use yhtye_core::usage::UsageWindowKind;
    let r = TempRepo::new();
    let mut cfg = core_config(
        &r,
        json!({"turns": []}),
        json!({"turns": []}),
        json!({"turns": []}),
    );
    cfg.usage = Some(fake_harness(json!({"turns": [
        {"match": "/usage", "actions": [{"message": USAGE_MARKDOWN}]}
    ]})));
    let core = Core::start(cfg).await.expect("core");

    let first = usage(&core, false).await;
    assert_eq!(first.plan.as_deref(), Some("max"));
    let five = first.window(UsageWindowKind::FiveHour).expect("5h");
    assert!((five.percent - 77.0).abs() < f64::EPSILON);
    assert!(five.resets_at_ms.is_some());
    let week = first.window(UsageWindowKind::Week).expect("week");
    assert!((week.percent - 40.0).abs() < f64::EPSILON);
    // Cached: neither a plain read nor an immediate refresh starts another agent.
    assert_eq!(usage(&core, false).await, first);
    assert_eq!(usage(&core, true).await, first);
    // The probe's agent was stopped (the data dir holds only its empty cwd).
    assert!(r.data.join("usage-probe").is_dir());
    core.shutdown().await;
}

#[tokio::test]
async fn usage_is_unavailable_rather_than_guessed() {
    let r = TempRepo::new();
    // No harness configured.
    let core = Core::start(core_config(
        &r,
        json!({"turns": []}),
        json!({"turns": []}),
        json!({"turns": []}),
    ))
    .await
    .expect("core");
    let cmd = ApiCommand::GetUsage { refresh: None };
    assert_eq!(
        error_code(&core, cmd.clone()).await,
        ApiErrorCode::Unavailable
    );
    core.shutdown().await;

    // The harness answers something that is not the usage Markdown.
    let other = TempRepo::new();
    let mut cfg = core_config(
        &other,
        json!({"turns": []}),
        json!({"turns": []}),
        json!({"turns": []}),
    );
    cfg.usage = Some(fake_harness(json!({"turns": [
        {"match": "/usage", "actions": [{"message": "Unknown command: /usage"}]}
    ]})));
    let core = Core::start(cfg).await.expect("core");
    let err = core.command(cmd).await.expect_err("unreadable");
    assert_eq!(err.code, ApiErrorCode::Unavailable);
    assert!(err.message.contains("could not be read"), "{err}");
    core.shutdown().await;
}

/// After `shutdown` nothing may start agents again (an open that raced the
/// shutdown would otherwise leave an orchestrator running after exit).
#[tokio::test]
async fn nothing_opens_after_shutdown() {
    let r = TempRepo::new();
    let mut cfg = core_config(
        &r,
        json!({"turns": []}),
        json!({"turns": []}),
        json!({"turns": []}),
    );
    cfg.usage = Some(fake_harness(json!({"turns": []})));
    let core = Core::start(cfg).await.expect("core");
    core.shutdown().await;
    let cmd = ApiCommand::OpenProject {
        path: r.repo.display().to_string(),
    };
    assert_eq!(error_code(&core, cmd).await, ApiErrorCode::Unavailable);
    let err = core
        .command(ApiCommand::GetUsage { refresh: None })
        .await
        .expect_err("closed");
    assert!(err.message.contains("shutting down"), "{err}");
}
