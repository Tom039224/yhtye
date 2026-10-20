//! Chat attachments through the API of [`Core`] with fake orchestrators
//! (`core-design.md` §19): staging, how each attachment reaches the agent
//! (image and text blocks, or only a path, by what the agent advertises in
//! `initialize`), the final decision being recorded in `prompted`, batches,
//! restarts and the errors.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::chats::Hold;
use common::fake_harness;
use common::repo::TempRepo;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use yhtye_core::agents::{AgentChoice, HarnessPreset};
use yhtye_core::api::{ApiCommand, ApiErrorCode, ApiEvent, ApiEventBody, ApiResponse, LoggedEvent};
use yhtye_core::attachments::{
    AttachmentDelivered, AttachmentDelivery as Delivery, AttachmentKind as Kind, AttachmentMeta,
    PromptCaps,
};
use yhtye_core::domain::DomainEvent;
use yhtye_core::runtime::{Core, CoreConfig};

const TIMEOUT: Duration = Duration::from_secs(30);
const ORCH: &str = "orchestrator:C-1";
/// The id the core gives the project in the temporary repository (its directory is `repo`).
const PROJECT: &str = "repo";

/// A PNG as far as the signature goes, with bytes that are not UTF-8.
fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend([0, 0xFF, 0xFE, 1, 2, 3, 0x80]);
    bytes
}

const NOTES: &str = "line one ✓\nline two\n";

fn fnv1a(data: &[u8]) -> String {
    let hash = data.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

fn core_config(r: &TempRepo, orch: Value) -> CoreConfig {
    let mut cfg = CoreConfig::claude_code(&r.data, "haiku");
    let none = fake_harness(json!({"turns": []}));
    cfg.harnesses = vec![HarnessPreset::fixed(
        "fake",
        fake_harness(orch),
        none.clone(),
        none,
    )];
    cfg.default_agent = AgentChoice::new("fake", Some("haiku"));
    cfg.usage = None;
    cfg
}

/// An orchestrator that accepts `caps` and reports the blocks of every prompt
/// that has an attachment line (the rest of the turns are given by `more`).
fn script(caps: Value, more: Vec<Value>) -> Value {
    let mut turns = vec![json!({"match": "[yhtye:attachment]", "actions": ["report_blocks"]})];
    turns.extend(more);
    json!({"prompt_capabilities": caps, "turns": turns})
}

fn all_caps() -> Value {
    json!({"image": true, "embedded_context": true})
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

async fn new_chat(core: &Core, project: &str) -> String {
    let cmd = ApiCommand::CreateChat {
        project: project.into(),
        branch: Some("main".into()),
        worktree: None,
    };
    match run(core, cmd).await {
        ApiResponse::Chat { chat } => chat.id,
        other => panic!("unexpected {other:?}"),
    }
}

fn stage_cmd(project: &str, chat: &str, name: &str, mime: Option<&str>, data: &[u8]) -> ApiCommand {
    ApiCommand::StageAttachment {
        project: project.into(),
        chat: chat.into(),
        name: name.into(),
        mime: mime.map(str::to_string),
        data_base64: STANDARD.encode(data),
    }
}

async fn stage(
    core: &Core,
    project: &str,
    name: &str,
    mime: Option<&str>,
    data: &[u8],
) -> AttachmentMeta {
    match run(core, stage_cmd(project, "C-1", name, mime, data)).await {
        ApiResponse::AttachmentStaged { attachment } => attachment,
        other => panic!("unexpected {other:?}"),
    }
}

fn send(project: &str, chat: &str, text: &str, attachments: &[&AttachmentMeta]) -> ApiCommand {
    ApiCommand::SendUserMessage {
        project: project.into(),
        chat: chat.into(),
        text: text.into(),
        attachments: attachments.iter().map(|a| a.id.clone()).collect(),
    }
}

async fn next<T>(rx: &mut broadcast::Receiver<ApiEvent>, f: impl Fn(&ApiEvent) -> Option<T>) -> T {
    loop {
        let ev = tokio::time::timeout(TIMEOUT, rx.recv())
            .await
            .expect("an event in time")
            .expect("event stream open");
        if let Some(found) = f(&ev) {
            return found;
        }
    }
}

/// The blocks the fake orchestrator reported (`report_blocks`).
async fn reported_blocks(rx: &mut broadcast::Receiver<ApiEvent>) -> Vec<Value> {
    let text = next(rx, |e| match &e.body {
        ApiEventBody::AgentText { session, text, .. }
            if session == ORCH && text.starts_with("blocks:") =>
        {
            Some(text.clone())
        }
        _ => None,
    })
    .await;
    match serde_json::from_str(&text["blocks:".len()..]).expect("blocks json") {
        Value::Array(blocks) => blocks,
        other => panic!("not an array: {other}"),
    }
}

/// What the orchestrator was prompted with: the text and the final deliveries.
async fn next_prompt(rx: &mut broadcast::Receiver<ApiEvent>) -> (String, Vec<AttachmentDelivered>) {
    next(rx, |e| match &e.body {
        ApiEventBody::Prompted {
            session,
            text,
            attachments,
        } if session == ORCH => Some((text.clone(), attachments.clone())),
        _ => None,
    })
    .await
}

fn of_type<'a>(blocks: &'a [Value], ty: &str) -> Vec<&'a Value> {
    blocks.iter().filter(|b| b["type"] == ty).collect()
}

fn file_of(r: &TempRepo, chat: &str, a: &AttachmentMeta) -> PathBuf {
    r.data
        .join("attachments")
        .join(PROJECT)
        .join(chat)
        .join(&a.id)
        .join(&a.name)
}

async fn events_of(core: &Core, project: &str) -> Vec<LoggedEvent> {
    let cmd = ApiCommand::ListEvents {
        project: project.into(),
        after_seq: 0,
        limit: Some(2000),
    };
    match run(core, cmd).await {
        ApiResponse::Events { events, .. } => events,
        other => panic!("unexpected {other:?}"),
    }
}

fn queued_item(events: &[LoggedEvent], inbox_id: u64) -> Value {
    events
        .iter()
        .find_map(|e| {
            let event = &e.body["event"];
            (e.body["type"] == "domain"
                && event["type"] == "inbox_queued"
                && event["entry"]["id"] == json!(inbox_id))
            .then(|| event["entry"]["item"].clone())
        })
        .unwrap_or_else(|| panic!("no inbox entry {inbox_id}"))
}

#[tokio::test]
async fn images_and_text_go_as_blocks_to_an_agent_that_accepts_them() {
    let r = TempRepo::new();
    let core = Core::start(core_config(&r, script(all_caps(), vec![])))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    new_chat(&core, &project).await;

    // Nothing is known of the agent yet: the prediction is "by path".
    let shot = stage(&core, &project, "shot.png", Some("image/png"), &png()).await;
    let notes = stage(&core, &project, "notes.txt", None, NOTES.as_bytes()).await;
    let report = stage(
        &core,
        &project,
        "report.pdf",
        Some("application/pdf"),
        b"%PDF-1.7\n",
    )
    .await;
    let blob = stage(&core, &project, "data.bin", None, &[0, 159, 146, 150]).await;
    assert_eq!(
        [&shot, &notes, &report, &blob].map(|a| (a.kind, a.delivery)),
        [
            (Kind::Image, Delivery::Path),
            (Kind::Text, Delivery::Path),
            (Kind::File, Delivery::Path),
            (Kind::File, Delivery::Path)
        ]
    );
    assert_eq!(
        (shot.mime.as_str(), shot.size, shot.name.as_str()),
        ("image/png", png().len() as u64, "shot.png")
    );
    assert_eq!(notes.mime, "text/plain");
    assert_eq!(
        std::fs::read(file_of(&r, "C-1", &shot)).expect("saved"),
        png()
    );

    run(
        &core,
        send(&project, "C-1", "look", &[&shot, &notes, &report, &blob]),
    )
    .await;
    let blocks = reported_blocks(&mut rx).await;

    // One text block, then the image and the text file as blocks of their own.
    assert_eq!(blocks.len(), 3, "{blocks:#?}");
    let text = blocks[0]["text"].as_str().expect("text");
    assert_eq!(blocks[0]["type"], "text");
    assert!(
        text.starts_with("[yhtye:user_message]\nlook\n[yhtye:attachment] name=shot.png"),
        "{text}"
    );
    let line_of = |name: &str| {
        text.lines()
            .find(|l| l.starts_with(&format!("[yhtye:attachment] name={name} ")))
            .unwrap_or_else(|| panic!("no line for {name} in {text}"))
            .to_string()
    };
    let shot_path = file_of(&r, "C-1", &shot);
    assert!(line_of("shot.png").contains(" mime=image/png "));
    assert!(line_of("shot.png").contains(" delivery=native "));
    assert!(line_of("shot.png").ends_with(&format!(" path={}", shot_path.display())));
    assert!(line_of("notes.txt").contains(" delivery=native "));
    assert!(line_of("report.pdf").contains(" mime=application/pdf "));
    assert!(line_of("report.pdf").contains(" delivery=path "));
    assert!(line_of("data.bin").contains(" delivery=path "));
    assert!(
        line_of("report.pdf")
            .ends_with(&format!(" path={}", file_of(&r, "C-1", &report).display()))
    );

    let image = &blocks[1];
    assert_eq!(image["type"], "image");
    assert_eq!(image["mime"], "image/png");
    assert_eq!(image["bytes"], json!(png().len()));
    assert_eq!(image["sha"], fnv1a(&png()).as_str());
    assert!(
        image["uri"]
            .as_str()
            .is_some_and(|u| u.starts_with("file:///") && u.ends_with("/shot.png")),
        "{image}"
    );
    let resource = &blocks[2];
    assert_eq!(resource["type"], "resource");
    assert_eq!(resource["text"], NOTES);
    assert_eq!(resource["mime"], "text/plain");
    assert!(
        resource["uri"]
            .as_str()
            .is_some_and(|u| u.ends_with("/notes.txt"))
    );

    // The final decision is in the event, the prediction in the queued message.
    let events = events_of(&core, &project).await;
    let prompted = events
        .iter()
        .find(|e| e.body["type"] == "prompted" && e.body["session"] == ORCH)
        .expect("prompted");
    let deliveries: Vec<(String, &str)> = prompted.body["attachments"]
        .as_array()
        .expect("attachments")
        .iter()
        .map(|a| {
            (
                a["id"].as_str().unwrap().to_string(),
                a["delivery"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        deliveries,
        [
            (shot.id.clone(), "native"),
            (notes.id.clone(), "native"),
            (report.id.clone(), "path"),
            (blob.id.clone(), "path"),
        ]
    );
    assert_eq!(prompted.body["attachments"][0]["inbox_id"], 1);
    let queued = queued_item(&events, 1);
    let queued_attachments = queued["attachments"].as_array().expect("attachments");
    assert_eq!(queued_attachments.len(), 4);
    assert_eq!(queued_attachments[0]["name"], "shot.png");
    assert_eq!(queued_attachments[0]["delivery"], "path", "the prediction");
    for a in queued_attachments {
        let keys: Vec<&str> = a.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys.len(),
            6,
            "id, name, mime, size, kind, delivery: {keys:?}"
        );
    }
    // The log holds no file content (the fake agent itself repeats the blocks
    // it got in a message of its own, which is not what Yhtye wrote).
    let ours: Vec<&Value> = events
        .iter()
        .map(|e| &e.body)
        .filter(|b| b["type"] != "agent_text")
        .collect();
    let log = serde_json::to_string(&ours).unwrap();
    assert!(
        !log.contains(&STANDARD.encode(png())),
        "base64 of the image in the log"
    );
    assert!(!log.contains("line one"), "text of the file in the log");

    // The orchestrator has said what it accepts: the next prediction is native.
    let again = stage(&core, &project, "again.png", Some("image/png"), &png()).await;
    assert_eq!(again.delivery, Delivery::Native);
    let again_txt = stage(&core, &project, "again.txt", None, b"x").await;
    assert_eq!(again_txt.delivery, Delivery::Native);
    core.shutdown().await;
}

#[tokio::test]
async fn an_agent_that_accepts_nothing_gets_every_attachment_by_path() {
    let r = TempRepo::new();
    let core = Core::start(core_config(&r, script(json!({}), vec![])))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    new_chat(&core, &project).await;
    let shot = stage(&core, &project, "shot.png", None, &png()).await;
    let notes = stage(&core, &project, "notes.md", None, b"# hi").await;

    run(&core, send(&project, "C-1", "see", &[&shot, &notes])).await;
    let blocks = reported_blocks(&mut rx).await;
    assert_eq!(blocks.len(), 1, "only the text: {blocks:#?}");
    let text = blocks[0]["text"].as_str().expect("text");
    assert_eq!(text.matches("delivery=path").count(), 2, "{text}");
    assert!(!text.contains("delivery=native"));
    let path = file_of(&r, "C-1", &shot);
    assert!(text.contains(&format!("path={}", path.display())), "{text}");
    assert_eq!(std::fs::read(&path).expect("the agent can read it"), png());
    let events = events_of(&core, &project).await;
    let prompted = events
        .iter()
        .find(|e| e.body["type"] == "prompted" && e.body["session"] == ORCH)
        .expect("prompted");
    assert!(
        prompted.body["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["delivery"] == "path")
    );
    // Prediction and final agree: the agent said nothing different.
    let again = stage(&core, &project, "again.png", None, &png()).await;
    assert_eq!(again.delivery, Delivery::Path);
    core.shutdown().await;
}

#[tokio::test]
async fn an_agent_that_only_sees_images_gets_text_files_by_path() {
    let r = TempRepo::new();
    let core = Core::start(core_config(&r, script(json!({"image": true}), vec![])))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    new_chat(&core, &project).await;
    let shot = stage(&core, &project, "shot.png", None, &png()).await;
    let notes = stage(&core, &project, "notes.txt", None, NOTES.as_bytes()).await;

    run(&core, send(&project, "C-1", "", &[&shot, &notes])).await;
    let blocks = reported_blocks(&mut rx).await;
    assert_eq!(of_type(&blocks, "image").len(), 1);
    assert!(of_type(&blocks, "resource").is_empty());
    let text = blocks[0]["text"].as_str().expect("text");
    assert!(
        text.starts_with("[yhtye:user_message]\n[yhtye:attachment] name=shot.png"),
        "{text}"
    );
    assert!(
        text.contains("name=notes.txt mime=text/plain size=22 delivery=path"),
        "{text}"
    );
    core.shutdown().await;
}

#[tokio::test]
async fn messages_queued_during_a_turn_keep_their_attachments_in_one_batch() {
    let r = TempRepo::new();
    let hold = Hold::new(&r, "release-first-turn");
    let more = vec![json!({"match": "work", "actions": [hold.action()]})];
    let core = Core::start(core_config(&r, script(all_caps(), more)))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    new_chat(&core, &project).await;

    run(&core, send(&project, "C-1", "work", &[])).await;
    let (first, deliveries) = next_prompt(&mut rx).await;
    assert_eq!(first, "[yhtye:user_message]\nwork");
    assert!(deliveries.is_empty());

    // The orchestrator is busy: both messages wait in the inbox.
    let a = stage(&core, &project, "a.png", None, &png()).await;
    let b = stage(&core, &project, "b.txt", None, b"bee").await;
    run(&core, send(&project, "C-1", "second", &[&a])).await;
    run(&core, send(&project, "C-1", "third", &[&b])).await;
    hold.release();

    let blocks = reported_blocks(&mut rx).await;
    let text = blocks[0]["text"].as_str().expect("text");
    let at = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("{needle} in {text}"))
    };
    assert!(at("second") < at("name=a.png"));
    assert!(at("name=a.png") < at("[yhtye:user_message]\nthird"));
    assert!(at("[yhtye:user_message]\nthird") < at("name=b.txt"));
    assert_eq!(of_type(&blocks, "image").len(), 1);
    assert_eq!(of_type(&blocks, "resource").len(), 1);
    assert_eq!(blocks[1]["type"], "image", "in the order of the messages");
    assert_eq!(blocks[2]["text"], "bee");

    let events = events_of(&core, &project).await;
    let batch = events
        .iter()
        .rev()
        .find(|e| e.body["type"] == "prompted" && e.body["session"] == ORCH)
        .expect("the batch");
    let owners: Vec<(u64, String)> = batch.body["attachments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["inbox_id"].as_u64().unwrap(),
                d["id"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(owners, [(2, a.id.clone()), (3, b.id.clone())]);
    core.shutdown().await;
}

#[tokio::test]
async fn attachments_still_arrive_when_the_orchestrator_is_restarted_without_its_session() {
    let r = TempRepo::new();
    let hold = Hold::new(&r, "never-released");
    let mut orch = script(
        all_caps(),
        vec![json!({"match": "work", "actions": [hold.action()]})],
    );
    orch["load_session"] = json!(false);

    let core = Core::start(core_config(&r, orch.clone()))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    new_chat(&core, &project).await;
    run(&core, send(&project, "C-1", "work", &[])).await;
    next_prompt(&mut rx).await;
    let shot = stage(&core, &project, "shot.png", None, &png()).await;
    let notes = stage(&core, &project, "notes.txt", None, NOTES.as_bytes()).await;
    run(
        &core,
        send(&project, "C-1", "after the stop", &[&shot, &notes]),
    )
    .await;
    // Yhtye stops while the message waits in the inbox.
    core.shutdown().await;

    let core = Core::start(core_config(&r, orch)).await.expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    run(
        &core,
        ApiCommand::OpenChat {
            project: project.clone(),
            chat: "C-1".into(),
        },
    )
    .await;
    let blocks = reported_blocks(&mut rx).await;
    let text = blocks[0]["text"].as_str().expect("text");
    assert!(
        text.contains("[yhtye:restarted]"),
        "the lost session is told: {text}"
    );
    assert!(text.contains("after the stop"), "{text}");
    assert!(text.contains("name=shot.png"), "{text}");
    assert_eq!(of_type(&blocks, "image").len(), 1, "{blocks:#?}");
    assert_eq!(of_type(&blocks, "resource").len(), 1, "{blocks:#?}");
    let image = of_type(&blocks, "image")[0];
    assert_eq!(image["sha"], fnv1a(&png()).as_str());
    core.shutdown().await;
}

#[tokio::test]
async fn staging_sending_and_discarding_have_clear_errors() {
    let r = TempRepo::new();
    let core = Core::start(core_config(&r, script(all_caps(), vec![])))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    new_chat(&core, &project).await;

    // Staging: unknown chat or project, bad base64.
    assert_eq!(
        code(&core, stage_cmd(&project, "C-9", "a.txt", None, b"x")).await,
        ApiErrorCode::NotFound
    );
    assert_eq!(
        code(&core, stage_cmd("nope", "C-1", "a.txt", None, b"x")).await,
        ApiErrorCode::NotFound
    );
    let bad = ApiCommand::StageAttachment {
        project: project.clone(),
        chat: "C-1".into(),
        name: "a.txt".into(),
        mime: None,
        data_base64: "not base64!".into(),
    };
    assert_eq!(code(&core, bad).await, ApiErrorCode::InvalidArgument);

    // A hostile name stays inside the attachment directory.
    let evil = stage(&core, &project, "../../../../tmp/evil.txt", None, b"x").await;
    assert_eq!(evil.name, "evil.txt");
    assert!(file_of(&r, "C-1", &evil).is_file());
    let empty_name = stage(&core, &project, "..", None, b"x").await;
    assert_eq!(empty_name.name, "attachment");
    let empty_file = stage(&core, &project, "empty.txt", None, b"").await;
    assert_eq!((empty_file.size, empty_file.kind), (0, Kind::Text));

    // Reading it back.
    let ApiResponse::Attachment {
        attachment,
        data_base64,
    } = run(
        &core,
        ApiCommand::GetAttachment {
            project: project.clone(),
            chat: "C-1".into(),
            id: evil.id.clone(),
        },
    )
    .await
    else {
        panic!("an attachment");
    };
    assert_eq!(
        (attachment, STANDARD.decode(data_base64).expect("b64")),
        (evil.clone(), b"x".to_vec())
    );

    // Discarding.
    let get = |id: &str| ApiCommand::GetAttachment {
        project: project.clone(),
        chat: "C-1".into(),
        id: id.into(),
    };
    let discard = |id: &str| ApiCommand::DiscardAttachment {
        project: project.clone(),
        chat: "C-1".into(),
        id: id.into(),
    };
    assert!(matches!(
        run(&core, discard(&empty_name.id)).await,
        ApiResponse::Accepted
    ));
    assert!(!file_of(&r, "C-1", &empty_name).exists());
    assert_eq!(
        code(&core, get(&empty_name.id)).await,
        ApiErrorCode::NotFound
    );
    assert_eq!(
        code(&core, discard(&empty_name.id)).await,
        ApiErrorCode::NotFound
    );
    assert_eq!(
        code(&core, discard("../x")).await,
        ApiErrorCode::InvalidArgument
    );

    // Sending: nothing at all, an unknown or discarded id, an id twice.
    assert_eq!(
        code(&core, send(&project, "C-1", "  ", &[])).await,
        ApiErrorCode::InvalidArgument
    );
    let ghost = AttachmentMeta {
        id: "0123456789abcdef".into(),
        ..evil.clone()
    };
    assert_eq!(
        code(&core, send(&project, "C-1", "x", &[&ghost])).await,
        ApiErrorCode::NotFound
    );
    assert_eq!(
        code(&core, send(&project, "C-1", "x", &[&empty_name])).await,
        ApiErrorCode::NotFound
    );
    assert_eq!(
        code(&core, send(&project, "C-1", "x", &[&evil, &evil])).await,
        ApiErrorCode::InvalidArgument
    );
    // Not sent by the failed attempts: it can still be discarded or sent.
    // A message with attachments and no text is fine.
    assert!(matches!(
        run(&core, send(&project, "C-1", "", &[&evil, &empty_file])).await,
        ApiResponse::Accepted
    ));
    let (text, deliveries) = next_prompt(&mut rx).await;
    assert!(
        text.starts_with("[yhtye:user_message]\n[yhtye:attachment] name=evil.txt"),
        "{text}"
    );
    assert_eq!(deliveries.len(), 2);

    // Once sent, an attachment stays and cannot be sent again.
    assert_eq!(
        code(&core, discard(&evil.id)).await,
        ApiErrorCode::InvalidState
    );
    assert_eq!(
        code(&core, send(&project, "C-1", "again", &[&evil])).await,
        ApiErrorCode::InvalidArgument
    );
    assert!(matches!(
        run(&core, get(&evil.id)).await,
        ApiResponse::Attachment { .. }
    ));
    assert!(file_of(&r, "C-1", &evil).is_file());

    // A message to a chat that is gone is refused, and its attachments stay unsent.
    let spare = stage(&core, &project, "spare.txt", None, b"s").await;
    assert_eq!(
        code(&core, send(&project, "C-7", "x", &[&spare])).await,
        ApiErrorCode::NotFound
    );
    assert!(matches!(
        run(&core, discard(&spare.id)).await,
        ApiResponse::Accepted
    ));
    core.shutdown().await;
}

#[tokio::test]
async fn deleting_a_chat_deletes_its_attachments() {
    let r = TempRepo::new();
    let core = Core::start(core_config(&r, script(all_caps(), vec![])))
        .await
        .expect("core");
    let project = open(&core, &r).await;
    new_chat(&core, &project).await;
    let kept = stage(&core, &project, "kept.txt", None, b"k").await;
    // Another chat's attachments are not touched.
    let second = {
        let cmd = ApiCommand::CreateChat {
            project: project.clone(),
            branch: Some("main".into()),
            worktree: None,
        };
        let ApiResponse::Chat { chat } = run(&core, cmd).await else {
            panic!("a chat")
        };
        chat.id
    };
    assert_eq!(second, "C-2");
    let other = match run(&core, stage_cmd(&project, "C-2", "other.txt", None, b"o")).await {
        ApiResponse::AttachmentStaged { attachment } => attachment,
        other => panic!("unexpected {other:?}"),
    };
    assert!(file_of(&r, "C-1", &kept).is_file());

    run(
        &core,
        ApiCommand::DeleteChat {
            project: project.clone(),
            chat: "C-1".into(),
        },
    )
    .await;
    assert!(
        !r.data
            .join("attachments")
            .join(PROJECT)
            .join("C-1")
            .exists()
    );
    assert!(file_of(&r, "C-2", &other).is_file());
    let get = ApiCommand::GetAttachment {
        project: project.clone(),
        chat: "C-1".into(),
        id: kept.id.clone(),
    };
    assert_eq!(code(&core, get).await, ApiErrorCode::NotFound);
    core.shutdown().await;
}

#[tokio::test]
async fn a_message_with_attachments_titles_an_untitled_chat() {
    let r = TempRepo::new();
    let core = Core::start(core_config(&r, script(all_caps(), vec![])))
        .await
        .expect("core");
    let mut rx = core.subscribe();
    let project = open(&core, &r).await;
    new_chat(&core, &project).await;
    let shot = stage(&core, &project, "design.png", None, &png()).await;
    run(&core, send(&project, "C-1", "", &[&shot])).await;
    let title = next(&mut rx, |e| match &e.body {
        ApiEventBody::Domain {
            event: DomainEvent::ChatTitled { title, .. },
        } => Some(title.clone()),
        _ => None,
    })
    .await;
    assert_eq!(title, "design.png");
    core.shutdown().await;
}

#[test]
fn caps_are_what_the_catalog_expects_before_a_harness_has_spoken() {
    use yhtye_core::agents::HarnessPreset;
    for preset in [
        HarnessPreset::claude_code("haiku"),
        HarnessPreset::codex(None),
    ] {
        assert_eq!(preset.prompt_caps, PromptCaps::ALL, "{}", preset.id);
    }
    for preset in [
        HarnessPreset::opencode("m", vec![]),
        HarnessPreset::devin("devin"),
        HarnessPreset::minimax_code("mcode"),
        HarnessPreset::antigravity("agy"),
        HarnessPreset::grok_build("grok"),
    ] {
        assert_eq!(preset.prompt_caps, PromptCaps::default(), "{}", preset.id);
    }
}
