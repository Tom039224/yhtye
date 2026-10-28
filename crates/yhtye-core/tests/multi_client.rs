//! Several clients on one [`Core`] (T-65, `core-design.md` §10): what one
//! client does reaches the others as app-wide events (`projects_changed`,
//! `settings_changed`), an old settings screen cannot overwrite newer settings
//! (`conflict`), concurrent changes leave the database and what is in effect
//! agreeing, and a session being started shows as starting to every client
//! (`session_starting`, `Snapshot::starting`).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::fake_harness;
use common::repo::TempRepo;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use yhtye_core::agents::{AgentChoice, AgentRole, HarnessPreset, RoleSettings};
use yhtye_core::api::{
    AgentSettingsView, ApiCommand, ApiErrorCode, ApiEvent, ApiEventBody, ApiResponse, SettingsKind,
    Snapshot,
};
use yhtye_core::runtime::{Core, CoreConfig};
use yhtye_core::secrets::{MemoryBackend, SecretValue};

const TIMEOUT: Duration = Duration::from_secs(30);

fn config(r: &TempRepo, orch: Value) -> CoreConfig {
    let mut cfg = CoreConfig::claude_code(&r.data, "haiku");
    let models = |script: Value| {
        let mut h = fake_harness(script);
        h.startup_timeout = Duration::from_secs(3);
        h
    };
    cfg.harnesses = vec![
        HarnessPreset::fixed(
            "fake",
            models(orch),
            models(json!({"turns": []})),
            models(json!({"turns": []})),
        ),
        HarnessPreset::fixed(
            "fake-b",
            models(json!({"turns": []})),
            models(json!({"turns": []})),
            models(json!({"turns": []})),
        ),
    ];
    cfg.default_agent = AgentChoice::new("fake", Some("haiku"));
    cfg.usage = None;
    cfg.secret_backend = Arc::new(MemoryBackend::default());
    cfg
}

async fn run(core: &Core, cmd: ApiCommand) -> ApiResponse {
    core.command(cmd.clone())
        .await
        .unwrap_or_else(|e| panic!("{cmd:?} failed: {e}"))
}

async fn view(core: &Core, project: Option<&str>) -> AgentSettingsView {
    let cmd = ApiCommand::GetAgentSettings {
        project: project.map(str::to_string),
    };
    match run(core, cmd).await {
        ApiResponse::AgentSettings { settings } => *settings,
        other => panic!("unexpected {other:?}"),
    }
}

fn set(harness: &str, expected: Option<u64>) -> ApiCommand {
    ApiCommand::SetAgentSettings {
        project: None,
        role: AgentRole::Implementer,
        settings: Some(RoleSettings::only(AgentChoice::new(harness, None))),
        expected_revision: expected,
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

/// The next event that `f` picks, skipping the others.
async fn next<T>(rx: &mut broadcast::Receiver<ApiEvent>, f: impl Fn(&ApiEvent) -> Option<T>) -> T {
    loop {
        let ev = tokio::time::timeout(TIMEOUT, rx.recv())
            .await
            .expect("an event in time")
            .expect("open");
        if let Some(found) = f(&ev) {
            return found;
        }
    }
}

/// The app-wide events received so far (without waiting).
fn app_wide_so_far(rx: &mut broadcast::Receiver<ApiEvent>) -> Vec<ApiEventBody> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if ev.body.is_app_wide() {
            assert!(ev.live && ev.seq == 0 && ev.project.is_empty(), "{ev:?}");
            out.push(ev.body);
        }
    }
    out
}

#[tokio::test]
async fn an_old_settings_screen_gets_a_conflict_and_changes_are_announced() {
    let r = TempRepo::new();
    let core = Core::start(config(&r, json!({"turns": []})))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    // Two screens read the same settings.
    let first = view(&core, None).await;
    let second = view(&core, None).await;
    assert_eq!(first.revision, second.revision);

    // The first saves: accepted, announced with the new revision.
    let ApiResponse::AgentSettings { settings: saved } =
        run(&core, set("fake-b", Some(first.revision))).await
    else {
        panic!("expected settings");
    };
    assert!(saved.revision > first.revision);
    let announced = next(&mut rx, |e| match &e.body {
        ApiEventBody::SettingsChanged {
            kind: SettingsKind::AgentSettings,
            project,
            revision,
        } => Some((project.clone(), *revision)),
        _ => None,
    })
    .await;
    assert_eq!(announced, (None, Some(saved.revision)));

    // The second, still showing the old settings, does not overwrite them.
    let err = core
        .command(set("fake", Some(second.revision)))
        .await
        .expect_err("stale");
    assert_eq!(err.code, ApiErrorCode::Conflict, "{err}");
    let now = view(&core, None).await;
    assert_eq!(now.revision, saved.revision, "nothing changed");
    assert_eq!(
        now.global.implementer.expect("set").default.harness,
        "fake-b"
    );
    // After reading again it can save; without a revision nothing is checked.
    run(&core, set("fake", Some(now.revision))).await;
    run(&core, set("fake-b", None)).await;
    core.shutdown().await;
}

#[tokio::test]
async fn concurrent_setting_changes_leave_the_database_and_memory_agreeing() {
    let r = TempRepo::new();
    let core = Core::start(config(&r, json!({"turns": []})))
        .await
        .expect("core");
    let start = view(&core, None).await.revision;
    let tasks: Vec<_> = (0..16)
        .map(|i| {
            let core = core.clone();
            let harness = if i % 2 == 0 { "fake" } else { "fake-b" };
            tokio::spawn(async move { core.command(set(harness, None)).await })
        })
        .collect();
    for t in tasks {
        t.await.expect("join").expect("set");
    }
    let after = view(&core, None).await;
    // Every change got its own revision.
    assert!(after.revision >= start + 16, "{} {}", start, after.revision);
    let in_memory = after.global.implementer.clone();
    core.shutdown().await;
    drop(core);
    // A new core reads the database: the same as what was in effect.
    let again = Core::start(config(&r, json!({"turns": []})))
        .await
        .expect("restart");
    let stored = view(&again, None).await;
    assert_eq!(stored.global.implementer, in_memory);
    assert!(stored.revision > after.revision, "newer across restarts");
    again.shutdown().await;
}

#[tokio::test]
async fn secret_and_project_changes_are_announced_to_every_client() {
    let r = TempRepo::new();
    let core = Core::start(config(&r, json!({"turns": []})))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let path = r.repo.display().to_string();
    let open = ApiCommand::OpenProject { path };
    let ApiResponse::Project { project } = run(&core, open.clone()).await else {
        panic!("expected a project");
    };
    // Opening it again (another client reopening it) changes nothing.
    run(&core, open).await;
    run(
        &core,
        ApiCommand::SetSecretEnv {
            name: "A_KEY".into(),
            value: SecretValue::new("v"),
        },
    )
    .await;
    run(
        &core,
        ApiCommand::DeleteSecretEnv {
            name: "A_KEY".into(),
        },
    )
    .await;
    // A refused change is not announced.
    let bad = ApiCommand::SetSecretEnv {
        name: "1BAD".into(),
        value: SecretValue::new("v"),
    };
    assert!(core.command(bad).await.is_err());
    // Let the broadcast deliver what was sent.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let secrets = ApiEventBody::SettingsChanged {
        kind: SettingsKind::Secrets,
        project: None,
        revision: None,
    };
    let got: Vec<String> = app_wide_so_far(&mut rx)
        .iter()
        .map(|b| serde_json::to_string(b).expect("json"))
        .collect();
    let want: Vec<String> = [
        ApiEventBody::ProjectsChanged {
            project: project.id.clone(),
        },
        secrets.clone(),
        secrets,
    ]
    .iter()
    .map(|b| serde_json::to_string(b).expect("json"))
    .collect();
    assert_eq!(got, want);
    core.shutdown().await;
}

#[tokio::test]
async fn every_client_sees_an_orchestrator_starting() {
    let r = TempRepo::new();
    // The orchestrator never finishes starting (until its start times out).
    let orch = json!({"hang_at": "initialize", "turns": []});
    let core = Core::start(config(&r, orch)).await.expect("core");
    let mut rx = core.subscribe();
    let path = r.repo.display().to_string();
    let ApiResponse::Project { project } = run(&core, ApiCommand::OpenProject { path }).await
    else {
        panic!("expected a project");
    };
    let project = project.id;
    let chat = ApiCommand::CreateChat {
        project: project.clone(),
        branch: Some("main".into()),
        worktree: None,
    };
    run(&core, chat).await;
    assert!(snapshot(&core, &project).await.starting.is_empty());

    // One client sends a message, which starts the chat's orchestrator.
    let send = ApiCommand::SendUserMessage {
        project: project.clone(),
        chat: "C-1".into(),
        text: "hello".into(),
        attachments: vec![],
    };
    run(&core, send).await;
    let starting = next(&mut rx, |e| match &e.body {
        ApiEventBody::SessionStarting { session } => Some((session.clone(), e.live)),
        _ => None,
    })
    .await;
    assert_eq!(starting, ("orchestrator:C-1".to_string(), true));
    // A client that connects now reads it from the snapshot.
    assert_eq!(
        snapshot(&core, &project).await.starting,
        ["orchestrator:C-1"]
    );
    // The start fails for good: it is starting no more.
    next(&mut rx, |e| match &e.body {
        ApiEventBody::SessionFailed {
            session,
            retrying: false,
            ..
        } if session == "orchestrator:C-1" => Some(()),
        _ => None,
    })
    .await;
    assert!(snapshot(&core, &project).await.starting.is_empty());
    core.shutdown().await;
}
