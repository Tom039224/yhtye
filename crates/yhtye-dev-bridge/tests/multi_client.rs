//! Two clients on one core over real WebSockets (T-65): what one does reaches
//! the other (the project list, the settings, the chats), a stale settings
//! screen gets `conflict`, and when both send the same cancel or delete at
//! once, both are answered `accepted` and it happens once.

mod common;

use common::{Bridge, Ws, connect, fake_core, reply, send, temp_repo, until_event};
use serde_json::{Value, json};

/// Sends `cmd` as request `id` (the reply is read later).
async fn request(ws: &mut Ws, id: u64, cmd: Value) {
    send(ws, json!({"id": id, "cmd": cmd}).to_string()).await;
}

/// Sends `cmd` and reads its reply.
async fn call(ws: &mut Ws, id: u64, cmd: Value, events: &mut Vec<Value>) -> Value {
    request(ws, id, cmd).await;
    reply(ws, id, events).await
}

fn domain_type(e: &Value) -> &str {
    e["body"]["event"]["type"].as_str().unwrap_or_default()
}

/// Both clients send `cmd` at once; both replies (events read meanwhile are kept).
async fn both(
    (a, ea): (&mut Ws, &mut Vec<Value>),
    (b, eb): (&mut Ws, &mut Vec<Value>),
    id: u64,
    cmd: Value,
) -> (Value, Value) {
    request(a, id, cmd.clone()).await;
    request(b, id, cmd).await;
    (reply(a, id, ea).await, reply(b, id, eb).await)
}

/// Waits for an event `f` picks, unless one was read already.
async fn seen_or_wait(ws: &mut Ws, events: &mut Vec<Value>, f: impl Fn(&Value) -> bool) {
    if !events.iter().any(&f) {
        until_event(ws, events, f).await;
    }
}

fn accepted(r: &(Value, Value)) -> bool {
    r.0["ok"]["type"] == "accepted" && r.1["ok"]["type"] == "accepted"
}

#[tokio::test]
async fn what_one_client_does_reaches_the_other() {
    let root = tempfile::tempdir().expect("tempdir");
    let (repo, data) = temp_repo(root.path());
    let orch = json!({"turns": [
        {"match": "start", "actions": [
            {"mcp_call": {"tool": "create_group", "args": {"title": "one"}}},
            {"mcp_call": {"tool": "create_task", "args": {"group_id": "${group_id}", "title": "a",
                "kind": "code", "steps": [{"kind": "implement"}], "instruction": "wait-a"}}},
            {"mcp_call": {"tool": "create_task", "args": {"group_id": "${group_id}", "title": "b",
                "kind": "code", "steps": [{"kind": "implement"}], "instruction": "wait-b"}}},
            {"message": "started"}
        ]},
        {"actions": [{"message": "noted"}]}
    ]});
    let core = fake_core(
        &data,
        orch,
        json!({"turns": [{"actions": ["wait_cancel"]}]}),
    )
    .await;
    let bridge = Bridge::start(&core).await;
    let mut a = connect(bridge.port, common::TOKEN, None).await.expect("a");
    let mut b = connect(bridge.port, common::TOKEN, None).await.expect("b");
    let (mut ea, mut eb) = (Vec::new(), Vec::new());

    // A opens a new project: B is told to re-read the project list.
    let open = json!({"type": "open_project", "path": repo.display().to_string()});
    let opened = call(&mut a, 1, open, &mut ea).await;
    let project = opened["ok"]["project"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    until_event(&mut b, &mut eb, |e| e["body"]["type"] == "projects_changed").await;
    let hint = eb.last().expect("hint");
    assert_eq!(hint["body"]["project"], project.as_str());
    assert_eq!(
        (
            hint["project"].clone(),
            hint["seq"].clone(),
            hint["live"].clone()
        ),
        (json!(""), json!(0), json!(true))
    );

    // Both show the settings; A saves, B hears of it, and B's old screen cannot overwrite.
    let get = json!({"type": "get_agent_settings"});
    let shown_a = call(&mut a, 2, get.clone(), &mut ea).await["ok"]["settings"]["revision"].clone();
    let shown_b = call(&mut b, 2, get, &mut eb).await["ok"]["settings"]["revision"].clone();
    assert_eq!(shown_a, shown_b);
    let save = |rev: &Value| {
        json!({"type": "set_agent_settings", "role": "implementer", "expected_revision": rev,
            "settings": {"candidates": [{"harness": "fake", "model": null}],
                "default": {"harness": "fake", "model": null}}})
    };
    let saved = call(&mut a, 3, save(&shown_a), &mut ea).await;
    let revision = saved["ok"]["settings"]["revision"].clone();
    assert!(revision.as_u64() > shown_a.as_u64(), "{saved}");
    until_event(&mut b, &mut eb, |e| e["body"]["type"] == "settings_changed").await;
    let changed = &eb.last().expect("hint")["body"];
    assert_eq!(
        (changed["kind"].clone(), changed["revision"].clone()),
        (json!("agent_settings"), revision)
    );
    let stale = call(&mut b, 3, save(&shown_b), &mut eb).await;
    assert_eq!(stale["err"]["code"], "conflict", "{stale}");

    // A makes a chat and starts a task in it: B sees both.
    let chat = json!({"type": "create_chat", "project": project, "branch": "main"});
    assert_eq!(
        call(&mut a, 4, chat, &mut ea).await["ok"]["chat"]["id"],
        "C-1"
    );
    until_event(&mut b, &mut eb, |e| domain_type(e) == "chat_created").await;
    let say =
        json!({"type": "send_user_message", "project": project, "chat": "C-1", "text": "start"});
    call(&mut a, 5, say, &mut ea).await;
    until_event(&mut b, &mut eb, |e| {
        domain_type(e) == "step_started" && e["body"]["event"]["task"] == "T-2"
    })
    .await;

    // Both cancel a task, then the group, at once: both accepted, each done once.
    let cancel = json!({"type": "cancel_task", "project": project, "task": "T-1"});
    let replies = both((&mut a, &mut ea), (&mut b, &mut eb), 6, cancel).await;
    assert!(accepted(&replies), "{replies:?}");
    let cancel = json!({"type": "cancel_group", "project": project, "group": "G-1"});
    let replies = both((&mut a, &mut ea), (&mut b, &mut eb), 7, cancel).await;
    assert!(accepted(&replies), "{replies:?}");
    // The orchestrator is told of the group once; then it is idle.
    let note = |e: &Value| {
        e["body"]["type"] == "prompted"
            && e["body"]["text"]
                .as_str()
                .is_some_and(|t| t.contains("cancelled group G-1"))
    };
    seen_or_wait(&mut b, &mut eb, note).await;
    let told = eb.iter().position(note).expect("told");
    // The orchestrator's own turn on that note ends (the cancelled tasks' agents end
    // turns of their own meanwhile: those say nothing about the chat).
    let turn_ended = |e: &Value| {
        e["body"]["type"] == "agent"
            && e["body"]["session"] == "orchestrator:C-1"
            && e["body"]["event"]["type"] == "turn_ended"
    };
    if !eb[told..].iter().any(turn_ended) {
        until_event(&mut b, &mut eb, turn_ended).await;
    }
    let user_calls = |events: &[Value], tool: &str| {
        events
            .iter()
            .filter(|e| e["body"]["type"] == "tool_called" && e["body"]["record"]["tool"] == tool)
            .count()
    };
    assert_eq!(
        user_calls(&eb, "cancel_task"),
        1,
        "the loser records nothing"
    );
    assert_eq!(
        user_calls(&eb, "cancel_group"),
        1,
        "the loser records nothing"
    );

    // Both delete the chat at once: both accepted; A sees it go.
    let delete = json!({"type": "delete_chat", "project": project, "chat": "C-1"});
    let replies = both((&mut a, &mut ea), (&mut b, &mut eb), 8, delete).await;
    assert!(accepted(&replies), "{replies:?}");
    seen_or_wait(&mut a, &mut ea, |e| domain_type(e) == "chat_deleted").await;

    drop(a);
    drop(b);
    bridge.stop().await;
    core.shutdown().await;
}
