//! Names as ids (`orchestration-model.md` §2.1 "名前"): validation, uniqueness
//! (in the state, of deleted chats, in git), branch names, and projects whose
//! log still has numbered `G-<n>` / `T-<n>` ids.

use serde_json::json;

use super::*;
use ErrorCode::*;

fn create_group(name: &str) -> Value {
    json!({"name": name, "title": "g"})
}

fn create_task(group: &str, name: &str) -> Value {
    json!({"group_id": group, "name": name, "title": "t", "kind": "code",
           "steps": [{"kind": "implement"}], "instruction": "do it"})
}

fn message_of(sim: &mut Sim, tool: ToolName, args: Value) -> ToolError {
    sim.tool(orch(), tool, args).expect_err("must fail")
}

#[test]
fn the_name_is_the_id_of_the_group_and_the_task() {
    let mut sim = Sim::new();
    let (group, _) = sim.orch_ok(ToolName::CreateGroup, create_group("headless-server"));
    assert_eq!(group["group_id"], json!("headless-server"));
    assert_eq!(group["group_branch"], json!("yhtye/headless-server"));
    let (task, _) = sim.orch_ok(
        ToolName::CreateTask,
        create_task("headless-server", "remote-icon"),
    );
    assert_eq!(task["task_id"], json!("remote-icon"));
    assert_eq!(sim.t("remote-icon").group, "headless-server");
    let (detail, _) = sim.orch_ok(ToolName::GetTask, json!({"task_id": "remote-icon"}));
    assert_eq!(
        detail["branch"],
        json!("yhtye/headless-server--remote-icon")
    );
}

#[test]
fn invalid_names_are_refused_with_the_rule() {
    let mut sim = Sim::new();
    for bad in ["", "Api", "a--b", "-a", "a-", "a_b", "G-1", &"x".repeat(33)] {
        let err = message_of(&mut sim, ToolName::CreateGroup, create_group(bad));
        assert_eq!(err.code, InvalidArgument, "{bad}");
        assert!(err.message.contains("kebab-case"), "{}", err.message);
    }
    sim.orch_ok(ToolName::CreateGroup, create_group("api"));
    for bad in ["T-2", "Login", "log in"] {
        let err = message_of(&mut sim, ToolName::CreateTask, create_task("api", bad));
        assert_eq!(err.code, InvalidArgument, "{bad}");
        assert!(err.message.contains("remote-icon"), "{}", err.message);
    }
    assert_eq!(
        sim.orch_err(ToolName::CreateGroup, create_group("branches")),
        Conflict,
        "reserved"
    );
}

#[test]
fn names_are_unique_among_all_groups_and_tasks() {
    let mut sim = Sim::new();
    sim.orch_ok(ToolName::CreateGroup, create_group("api"));
    sim.orch_ok(ToolName::CreateTask, create_task("api", "login"));
    // A task name, also of a task in another group, or a group's name.
    for name in ["login", "api"] {
        let err = message_of(&mut sim, ToolName::CreateTask, create_task("api", name));
        assert_eq!(err.code, Conflict, "{name}");
        assert!(
            err.message.contains("choose another name"),
            "{}",
            err.message
        );
    }
    sim.orch_ok(
        ToolName::CancelGroup,
        json!({"group_id": "api", "reason": "x"}),
    );
    // A finished group keeps its name, in any chat.
    let c2 = sim.chat("feature");
    for name in ["api", "login"] {
        let err = sim
            .tool(orch_of(&c2), ToolName::CreateGroup, create_group(name))
            .expect_err("taken");
        assert_eq!(err.code, Conflict, "{name}");
    }
    sim.tool(orch_of(&c2), ToolName::CreateGroup, create_group("web"))
        .expect("free");
    let err = sim
        .tool(
            orch_of(&c2),
            ToolName::CreateTask,
            create_task("web", "login"),
        )
        .expect_err("taken in another group");
    assert_eq!(err.code, Conflict);
}

#[test]
fn names_of_a_deleted_chat_are_never_given_again() {
    let mut sim = Sim::new();
    let c2 = sim.chat("feature");
    sim.tool(orch_of(&c2), ToolName::CreateGroup, create_group("api"))
        .expect("created");
    sim.tool(
        orch_of(&c2),
        ToolName::CreateTask,
        create_task("api", "login"),
    )
    .expect("created");
    sim.tool(
        orch_of(&c2),
        ToolName::CancelGroup,
        json!({"group_id": "api", "reason": "x"}),
    )
    .expect("cancelled");
    sim.run(DomainCommand::DeleteChat { chat: c2 })
        .expect("deleted");
    assert!(sim.state.group("api").is_none());
    assert_eq!(sim.state.counters.retired_names, ["api", "login"]);

    sim.orch_ok(ToolName::CreateGroup, create_group("work"));
    for (tool, args) in [
        (ToolName::CreateGroup, create_group("api")),
        (ToolName::CreateTask, create_task("work", "login")),
        (ToolName::CreateTask, create_task("work", "api")),
    ] {
        let err = message_of(&mut sim, tool, args);
        assert_eq!(err.code, Conflict, "{}", err.message);
    }
}

#[test]
fn a_name_git_already_uses_is_refused_as_given() {
    let mut sim = Sim::new();
    let cmd = DomainCommand::CreateGroup {
        chat: "C-1".into(),
        args: crate::mcp::tools::CreateGroupArgs {
            name: "api".into(),
            title: "g".into(),
            summary: None,
        },
        base_branch: "main".into(),
        taken_in_git: Some("git already has the branch yhtye/api".into()),
    };
    let err = sim.run(cmd).expect_err("taken in git");
    assert_eq!(err.code, Conflict);
    assert!(
        err.message.contains("yhtye/api") && err.message.contains("choose another name"),
        "{}",
        err.message
    );
    assert!(sim.state.groups.is_empty(), "no number is appended");

    sim.group();
    let args = parse_call(
        ToolName::CreateTask,
        create_task("g1", "login").as_object().cloned(),
    );
    let Ok(crate::mcp::ToolCall::CreateTask(args)) = args else {
        panic!("valid args")
    };
    let cmd = DomainCommand::CreateTask {
        binding: orch(),
        args,
        taken_in_git: Some("the worktree directory /data/g1/login already exists".into()),
    };
    let err = sim.run(cmd).expect_err("taken in git");
    assert_eq!(err.code, Conflict);
    assert!(sim.state.tasks.is_empty());
}

#[test]
fn branches_keep_the_old_form_for_numbered_tasks_only() {
    assert_eq!(group_branch("api"), "yhtye/api");
    assert_eq!(group_branch("G-46"), "yhtye/G-46");
    assert_eq!(task_branch("api", "login"), "yhtye/api--login");
    assert_eq!(task_branch("G-46", "T-99"), "yhtye/G-46-T-99");
    assert_eq!(
        task_branch("G-46", "remote-icon"),
        "yhtye/G-46--remote-icon"
    );
}

/// What an older Yhtye logged for group `G-46` with the running `code` task
/// `T-99` (ids were numbers then).
fn old_log() -> Vec<DomainEvent> {
    let events = json!([
        {"type": "group_created", "group": {
            "id": "G-46", "chat": "C-1", "title": "old work", "summary": null,
            "base_branch": "main", "group_branch": "yhtye/G-46", "status": "active",
            "finish_summary": null, "detail": null, "finish_nudges": 0}},
        {"type": "task_created", "task": {
            "id": "T-99", "group": "G-46", "title": "old task", "kind": "code",
            "depends_on": [], "instruction": "do it",
            "steps": [
                {"kind": "implement", "instruction": null, "status": "pending", "result": null,
                 "verdict": null, "note": null, "nudges": 0},
                {"kind": "done", "instruction": null, "status": "pending", "result": null,
                 "verdict": null, "note": null, "nudges": 0}],
            "current": 0, "status": "pending", "review_rounds": 0, "workdir": null,
            "cancel_reason": null}},
        {"type": "task_status_changed", "task": "T-99", "status": "running"},
        {"type": "workspace_ready", "task": "T-99", "path": "/wt/G-46/T-99"},
        {"type": "step_started", "task": "T-99", "step": 0, "note": null}
    ]);
    serde_json::from_value(events).expect("the old log still parses")
}

#[test]
fn an_old_group_with_numbered_ids_goes_on_and_finishes() {
    let mut sim = Sim::new();
    for event in old_log() {
        sim.state.apply(&event);
    }
    assert_eq!(sim.status("T-99"), TaskStatus::Running);
    assert_eq!(sim.state.counters.groups, 46, "as an older Yhtye counted");
    let (detail, _) = sim.orch_ok(ToolName::GetTask, json!({"task_id": "T-99"}));
    assert_eq!(detail["branch"], json!("yhtye/G-46-T-99"));

    // A new, named task in the old group: the new branch form.
    sim.orch_ok(ToolName::CreateTask, create_task("G-46", "remote-icon"));
    let (detail, _) = sim.orch_ok(ToolName::GetTask, json!({"task_id": "remote-icon"}));
    assert_eq!(detail["branch"], json!("yhtye/G-46--remote-icon"));

    // Both finish; the git ops name the old ids as they are.
    let chain = sim.report("T-99", Role::Implementer, None);
    assert!(has_effect(&chain, |e| matches!(e,
        Effect::Git(GitOp::FinishTask { group, task, group_branch, .. })
            if group == "G-46" && task == "T-99" && group_branch == "yhtye/G-46")));
    sim.report("remote-icon", Role::Implementer, None);
    assert_eq!(sim.status("T-99"), TaskStatus::Done);
    assert_eq!(sim.status("remote-icon"), TaskStatus::Done);

    let (_, chain) = sim.orch_ok(
        ToolName::FinishGroup,
        json!({"group_id": "G-46", "summary": "done"}),
    );
    assert!(has_effect(&chain, |e| matches!(e,
        Effect::Git(GitOp::MergeGroup { group, group_branch, .. })
            if group == "G-46" && group_branch == "yhtye/G-46")));
    assert_eq!(
        sim.state.group("G-46").map(|g| g.status),
        Some(GroupStatus::Done)
    );

    // The next group is named; nothing in the old counters is in the way.
    sim.orch_ok(ToolName::CreateGroup, create_group("next-work"));
    assert_eq!(sim.state.counters.groups, 47);
}
