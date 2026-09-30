//! The chat API of [`Core`] (`core-design.md` §17.5) on a temporary git
//! repository with fake agents: listing, creating chats and branches and their
//! errors, sending to chats, and resuming projects with live chats.

mod common;

use std::time::Duration;

use common::fake_harness;
use common::repo::TempRepo;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use yhtye_core::agents::{AgentChoice, HarnessPreset};
use yhtye_core::api::{ApiCommand, ApiErrorCode, ApiEvent, ApiEventBody, ApiResponse};
use yhtye_core::runtime::{Core, CoreConfig};
use yhtye_core::store::ChatInfo;

const TIMEOUT: Duration = Duration::from_secs(20);

fn core_config(r: &TempRepo, orch: Value) -> CoreConfig {
    let mut cfg = CoreConfig::claude_code(&r.data, "haiku");
    cfg.harnesses = vec![HarnessPreset::fixed(
        "fake",
        fake_harness(orch),
        fake_harness(json!({"turns": []})),
        fake_harness(json!({"turns": []})),
    )];
    cfg.default_agent = AgentChoice::new("fake", Some("haiku"));
    cfg.usage = None;
    cfg
}

fn hello_script() -> Value {
    json!({"turns": [{"match": "hello", "actions": [{"message": "hi"}]}]})
}

async fn run(core: &Core, cmd: ApiCommand) -> ApiResponse {
    core.command(cmd.clone())
        .await
        .unwrap_or_else(|e| panic!("{cmd:?} failed: {e}"))
}

async fn code(core: &Core, cmd: ApiCommand) -> ApiErrorCode {
    match core.command(cmd.clone()).await {
        Ok(r) => panic!("{cmd:?} succeeded: {r:?}"),
        Err(e) => e.code,
    }
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

async fn create_chat(core: &Core, project: &str, branch: &str) -> ChatInfo {
    let cmd = ApiCommand::CreateChat {
        project: project.into(),
        branch: branch.into(),
    };
    match run(core, cmd).await {
        ApiResponse::Chat { chat } => chat,
        other => panic!("unexpected {other:?}"),
    }
}

async fn list_chats(core: &Core, project: &str) -> Vec<ChatInfo> {
    let cmd = ApiCommand::ListChats {
        project: project.into(),
    };
    match run(core, cmd).await {
        ApiResponse::Chats { chats } => chats,
        other => panic!("unexpected {other:?}"),
    }
}

fn send(project: &str, chat: &str, text: &str) -> ApiCommand {
    ApiCommand::SendUserMessage {
        project: project.into(),
        chat: chat.into(),
        text: text.into(),
    }
}

async fn until(rx: &mut broadcast::Receiver<ApiEvent>, done: impl Fn(&ApiEvent) -> bool) {
    loop {
        let ev = tokio::time::timeout(TIMEOUT, rx.recv())
            .await
            .expect("timed out waiting for an event")
            .expect("event stream open");
        if done(&ev) {
            return;
        }
    }
}

fn said(session: &'static str, needle: &'static str) -> impl Fn(&ApiEvent) -> bool {
    move |e| {
        matches!(&e.body, ApiEventBody::AgentText { session: s, text, .. }
        if s == session && text.contains(needle))
    }
}

#[tokio::test]
async fn chats_are_created_listed_and_used_through_the_api() {
    let r = TempRepo::new();
    r.git(&["branch", "topic"]);
    let core = Core::start(core_config(&r, hello_script()))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    assert!(list_chats(&core, &project).await.is_empty());

    let first = create_chat(&core, &project, "main").await;
    let second = create_chat(&core, &project, "topic").await;
    assert_eq!(
        (
            first.id.as_str(),
            first.branch.as_str(),
            first.title.as_deref()
        ),
        ("C-1", "main", None)
    );
    assert_eq!(second.id, "C-2");
    assert_eq!(second.created_ms, second.last_used_ms);
    let ids: Vec<String> = list_chats(&core, &project)
        .await
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(ids, ["C-2", "C-1"], "the newest first while none was used");

    // Using the older chat brings it to the front and gives it its title.
    run(&core, send(&project, "C-1", "hello there, orchestrator")).await;
    until(&mut rx, said("orchestrator:C-1", "hi")).await;
    let chats = list_chats(&core, &project).await;
    assert_eq!(chats[0].id, "C-1");
    assert_eq!(chats[0].title.as_deref(), Some("hello there, orchestrator"));
    assert!(chats[0].last_used_ms >= chats[1].last_used_ms);
    let ApiResponse::Snapshot { snapshot } = run(
        &core,
        ApiCommand::GetSnapshot {
            project: project.clone(),
        },
    )
    .await
    else {
        panic!("a snapshot");
    };
    assert_eq!(snapshot.chats, chats, "the snapshot carries the same list");
    assert_eq!(snapshot.state.chats.len(), 2);

    // The list is readable while the project is closed; creating is not.
    core.shutdown().await;
    let core = Core::start(core_config(&r, hello_script()))
        .await
        .expect("core");
    assert_eq!(list_chats(&core, &project).await, chats);
    let create = ApiCommand::CreateChat {
        project: project.clone(),
        branch: "main".into(),
    };
    assert_eq!(code(&core, create).await, ApiErrorCode::Unavailable);
    let unknown = ApiCommand::ListChats {
        project: "nope".into(),
    };
    assert_eq!(code(&core, unknown).await, ApiErrorCode::NotFound);
    core.shutdown().await;
}

#[tokio::test]
async fn creating_chats_and_sending_report_bad_input() {
    let r = TempRepo::new();
    r.git(&["branch", "yhtye/G-1"]);
    let core = Core::start(core_config(&r, hello_script()))
        .await
        .expect("core");
    let project = open(&core, &r).await;
    let create = |branch: &str| ApiCommand::CreateChat {
        project: project.clone(),
        branch: branch.into(),
    };
    assert_eq!(code(&core, create("nope")).await, ApiErrorCode::NotFound);
    assert_eq!(
        code(&core, create("yhtye/G-1")).await,
        ApiErrorCode::InvalidArgument
    );
    assert_eq!(
        code(&core, create(" ")).await,
        ApiErrorCode::InvalidArgument
    );
    assert!(
        list_chats(&core, &project).await.is_empty(),
        "nothing was created"
    );

    assert_eq!(
        code(&core, send(&project, "C-9", "hi")).await,
        ApiErrorCode::NotFound
    );
    assert_eq!(
        code(&core, send(&project, "C-9", "  ")).await,
        ApiErrorCode::InvalidArgument
    );
    // Cancelling an unknown chat is refused.
    let cancel = ApiCommand::CancelOrchestratorTurn {
        project: project.clone(),
        chat: "C-9".into(),
    };
    assert_eq!(code(&core, cancel).await, ApiErrorCode::NotFound);

    // A chat whose branch was deleted can be read but not written to.
    r.git(&["branch", "topic"]);
    create_chat(&core, &project, "topic").await;
    r.git(&["branch", "-D", "topic"]);
    assert_eq!(
        code(&core, send(&project, "C-1", "hi")).await,
        ApiErrorCode::InvalidState
    );
    core.shutdown().await;
}

#[tokio::test]
async fn a_new_branch_gets_its_own_worktree_and_a_first_chat() {
    let r = TempRepo::new();
    let core = Core::start(core_config(&r, hello_script()))
        .await
        .expect("core");
    let project = open(&core, &r).await;
    let create = |name: &str, from: Option<&str>| ApiCommand::CreateBranch {
        project: project.clone(),
        name: name.into(),
        from: from.map(str::to_string),
    };
    let ApiResponse::Chat { chat } = run(&core, create("feature/new", None)).await else {
        panic!("a chat");
    };
    assert_eq!(
        (chat.id.as_str(), chat.branch.as_str()),
        ("C-1", "feature/new")
    );
    assert!(r.branch_exists("feature/new"));
    assert_eq!(
        r.git(&["branch", "--show-current"]).trim(),
        "main",
        "the main clone is not switched"
    );
    assert_eq!(r.extra_worktrees().len(), 1);
    assert_eq!(
        r.git(&["rev-parse", "feature/new"]),
        r.git(&["rev-parse", "main"])
    );
    assert_eq!(list_chats(&core, &project).await.len(), 1);

    assert_eq!(
        code(&core, create("feature/new", None)).await,
        ApiErrorCode::Conflict
    );
    assert_eq!(
        code(&core, create("main", None)).await,
        ApiErrorCode::Conflict
    );
    assert_eq!(
        code(&core, create("bad name", None)).await,
        ApiErrorCode::InvalidArgument
    );
    assert_eq!(
        code(&core, create("yhtye/x", None)).await,
        ApiErrorCode::InvalidArgument
    );
    assert_eq!(
        code(&core, create("other", Some("no-such"))).await,
        ApiErrorCode::NotFound
    );
    assert!(!r.branch_exists("other"));
    assert_eq!(
        list_chats(&core, &project).await.len(),
        1,
        "a failed create makes no chat"
    );
    core.shutdown().await;
}

#[tokio::test]
async fn a_project_whose_chat_was_live_is_reopened_and_its_orchestrator_restored() {
    let r = TempRepo::new();
    let core = Core::start(core_config(&r, hello_script()))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    create_chat(&core, &project, "main").await;
    run(&core, send(&project, "C-1", "hello")).await;
    until(&mut rx, said("orchestrator:C-1", "hi")).await;
    core.shutdown().await;

    let core = Core::start(core_config(&r, hello_script()))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let resumed = core.resume_unfinished().await;
    assert_eq!(resumed.len(), 1, "the project has a live chat");
    until(&mut rx, |e| {
        matches!(&e.body, ApiEventBody::SessionStarted { session, resumed: true, .. }
            if session == "orchestrator:C-1")
    })
    .await;
    core.shutdown().await;
}

#[tokio::test]
async fn a_project_whose_only_chat_ended_on_its_own_is_not_reopened() {
    let r = TempRepo::new();
    let script = json!({"turns": [{"match": "die", "actions": [{"crash": 3}]}]});
    let core = Core::start(core_config(&r, script.clone()))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    create_chat(&core, &project, "main").await;
    run(&core, send(&project, "C-1", "die")).await;
    until(&mut rx, |e| {
        matches!(&e.body, ApiEventBody::SessionStopped { session, suspended: false }
            if session == "orchestrator:C-1")
    })
    .await;
    core.shutdown().await;

    let core = Core::start(core_config(&r, script)).await.expect("core");
    assert!(
        core.resume_unfinished().await.is_empty(),
        "nothing was left to continue"
    );
    core.shutdown().await;
}
