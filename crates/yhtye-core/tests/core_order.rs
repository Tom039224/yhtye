//! The recently-used order of projects and branches through the API of
//! [`Core`] (`order.rs`): using an item moves it to the front, a reorder stores
//! the user's order, the order survives a restart, and every client is told.

mod common;

use std::time::Duration;

use common::fake_harness;
use common::repo::TempRepo;
use serde_json::json;
use tokio::sync::broadcast;
use yhtye_core::agents::{AgentChoice, HarnessPreset};
use yhtye_core::api::{ApiCommand, ApiErrorCode, ApiEvent, ApiEventBody, ApiResponse};
use yhtye_core::runtime::{Core, CoreConfig};

fn config(r: &TempRepo) -> CoreConfig {
    let mut cfg = CoreConfig::claude_code(&r.data, "haiku");
    let script =
        fake_harness(json!({"turns": [{"match": "hello", "actions": [{"message": "hi"}]}]}));
    cfg.harnesses = vec![HarnessPreset::fixed(
        "fake",
        script,
        fake_harness(json!({"turns": []})),
        fake_harness(json!({"turns": []})),
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

async fn open(core: &Core, path: &std::path::Path) -> String {
    let cmd = ApiCommand::OpenProject {
        path: path.display().to_string(),
    };
    match run(core, cmd).await {
        ApiResponse::Project { project } => project.id,
        other => panic!("unexpected {other:?}"),
    }
}

async fn project_ids(core: &Core) -> Vec<String> {
    match run(core, ApiCommand::ListProjects).await {
        ApiResponse::Projects { projects } => projects.into_iter().map(|p| p.id).collect(),
        other => panic!("unexpected {other:?}"),
    }
}

async fn branch_order(core: &Core, project: &str) -> Vec<String> {
    let cmd = ApiCommand::GetBranchOrder {
        project: project.into(),
    };
    match run(core, cmd).await {
        ApiResponse::BranchOrder { keys } => keys,
        other => panic!("unexpected {other:?}"),
    }
}

fn strings(keys: &[&str]) -> Vec<String> {
    keys.iter().map(ToString::to_string).collect()
}

/// The `branch_order_changed` events received so far.
fn branch_orders(rx: &mut broadcast::Receiver<ApiEvent>) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let ApiEventBody::BranchOrderChanged { project, keys } = ev.body {
            out.push((project, keys));
        }
    }
    out
}

/// A second repository registered in the same Core, as another project.
fn second_repo() -> TempRepo {
    TempRepo::new()
}

#[tokio::test]
async fn opening_a_project_moves_it_to_the_front_and_the_order_survives_a_restart() {
    let (a, b) = (TempRepo::new(), second_repo());
    let core = Core::start(config(&a)).await.expect("core");
    let mut rx = core.subscribe();
    let first = open(&core, &a.repo).await;
    let second = open(&core, &b.repo).await;
    // Both directories are called `repo`: the second one got `repo-2`.
    assert_ne!(first, second);
    assert_eq!(project_ids(&core).await, [second.clone(), first.clone()]);

    // Going back to the first one (already open) brings it to the front.
    open(&core, &a.repo).await;
    assert_eq!(project_ids(&core).await, [first.clone(), second.clone()]);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut changed = 0;
    while let Ok(ev) = rx.try_recv() {
        if matches!(ev.body, ApiEventBody::ProjectsChanged { .. }) {
            changed += 1;
        }
    }
    assert!(
        changed >= 3,
        "every open that changed the list was announced: {changed}"
    );

    // Opening the project that is already first changes nothing and says nothing.
    open(&core, &a.repo).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(rx.try_recv().is_err());
    core.shutdown().await;

    let core = Core::start(config(&a)).await.expect("core");
    assert_eq!(project_ids(&core).await, [first, second]);
    core.shutdown().await;
}

#[tokio::test]
async fn projects_are_reordered_by_hand_and_unknown_ids_are_ignored() {
    let (a, b) = (TempRepo::new(), second_repo());
    let core = Core::start(config(&a)).await.expect("core");
    let first = open(&core, &a.repo).await;
    let second = open(&core, &b.repo).await;
    let mut rx = core.subscribe();

    let reorder = ApiCommand::ReorderProjects {
        ids: vec![first.clone(), "ghost".into()],
    };
    let ApiResponse::Projects { projects } = run(&core, reorder).await else {
        panic!("a listing");
    };
    let ids: Vec<&str> = projects.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, [first.as_str(), second.as_str()]);
    assert_eq!(project_ids(&core).await, [first.clone(), second.clone()]);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let announced = std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|e| matches!(e.body, ApiEventBody::ProjectsChanged { .. }))
        .count();
    assert_eq!(announced, 1);
    core.shutdown().await;
}

#[tokio::test]
async fn using_a_chat_moves_its_worktree_to_the_front_and_a_reorder_is_stored() {
    let r = TempRepo::new();
    r.git(&["branch", "topic"]);
    let core = Core::start(config(&r)).await.expect("core");
    let project = open(&core, &r.repo).await;
    assert!(branch_order(&core, &project).await.is_empty());
    let mut rx = core.subscribe();

    let create = |branch: &str| ApiCommand::CreateChat {
        project: project.clone(),
        branch: Some(branch.into()),
        worktree: None,
    };
    let ApiResponse::Chat { chat: main_chat } = run(&core, create("main")).await else {
        panic!("a chat");
    };
    let ApiResponse::Chat { chat: topic_chat } = run(&core, create("topic")).await else {
        panic!("a chat");
    };
    // Creating a chat is using its branch: the newest worktree is first.
    let order = branch_order(&core, &project).await;
    assert_eq!(
        order,
        [topic_chat.worktree.clone(), main_chat.worktree.clone()]
    );

    // Opening the other chat brings its worktree back to the front.
    let open_chat = ApiCommand::OpenChat {
        project: project.clone(),
        chat: main_chat.id.clone(),
    };
    run(&core, open_chat).await;
    let order = branch_order(&core, &project).await;
    assert_eq!(
        order,
        [main_chat.worktree.clone(), topic_chat.worktree.clone()]
    );

    // Sending a message uses the chat too.
    let send = ApiCommand::SendUserMessage {
        project: project.clone(),
        chat: topic_chat.id.clone(),
        text: "hello".into(),
        attachments: vec![],
    };
    run(&core, send).await;
    let order = branch_order(&core, &project).await;
    assert_eq!(
        order,
        [topic_chat.worktree.clone(), main_chat.worktree.clone()]
    );

    // Every use (two creations, an open, a message) was announced with the whole
    // order; the last one is the current one.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let announced = branch_orders(&mut rx);
    assert_eq!(announced.len(), 4);
    assert_eq!(announced[3], (project.clone(), order.clone()));

    // A drag stores the user's order; keys it does not name keep their place after it.
    let reorder = ApiCommand::ReorderBranches {
        project: project.clone(),
        keys: strings(&["other-branch", &main_chat.worktree]),
    };
    let ApiResponse::BranchOrder { keys } = run(&core, reorder).await else {
        panic!("an order");
    };
    assert_eq!(
        keys,
        [
            "other-branch".to_string(),
            main_chat.worktree.clone(),
            topic_chat.worktree.clone()
        ]
    );
    assert_eq!(
        branch_orders(&mut rx).last().map(|(_, k)| k.clone()),
        Some(keys.clone())
    );

    // Putting it in the order it already has says nothing.
    let same = ApiCommand::ReorderBranches {
        project: project.clone(),
        keys: keys.clone(),
    };
    run(&core, same).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(branch_orders(&mut rx).is_empty());
    core.shutdown().await;

    // The order is stored, and readable while the project is closed.
    let core = Core::start(config(&r)).await.expect("core");
    assert_eq!(branch_order(&core, &project).await, keys);
    let unknown = ApiCommand::GetBranchOrder {
        project: "nope".into(),
    };
    let err = core.command(unknown).await.expect_err("unknown project");
    assert_eq!(err.code, ApiErrorCode::NotFound);
    core.shutdown().await;
}
