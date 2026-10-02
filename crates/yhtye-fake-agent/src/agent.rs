//! ACP agent-side handlers of the fake agent.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, ContentBlock, InitializeRequest, InitializeResponse,
    LoadSessionRequest, LoadSessionResponse, McpCapabilities, McpServer, NewSessionRequest,
    NewSessionResponse, PermissionOption, PermissionOptionKind, PromptRequest, PromptResponse,
    RequestPermissionOutcome, RequestPermissionRequest, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigSelectOption, SessionMode, SessionModeState,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
    SetSessionModeResponse, StopReason, ToolCallUpdate, ToolCallUpdateFields,
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Error, Stdio, UntypedMessage};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::watch;

use crate::mcp;
use crate::scenario::{Action, FailKind, PermissionChoice, Scenario};

/// The id of the effort option unless the scenario names another.
const DEFAULT_EFFORT_ID: &str = "effort";

/// Raw `session/update` so any update JSON can be sent.
#[derive(Debug, Clone, Serialize, Deserialize, agent_client_protocol::JsonRpcNotification)]
#[notification(method = "session/update")]
struct RawSessionUpdate(serde_json::Value);

/// Devin's vendor request asking the client for diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize, agent_client_protocol::JsonRpcRequest)]
#[request(method = "_cognition.ai/request_diagnostics", response = serde_json::Value)]
struct RequestDiagnostics(serde_json::Value);

#[derive(Default)]
struct State {
    session_id: String,
    mode: String,
    model: String,
    /// Current value of the `effort` option (when the model has one).
    effort: String,
    system_prompt: Option<String>,
    /// `clientInfo` of `initialize` as `(name, version)`.
    client_info: Option<(String, String)>,
    /// Full `_meta` of `session/new` / `session/load`.
    meta: Option<serde_json::Map<String, serde_json::Value>>,
    next_seq: usize,
    /// URL of the first HTTP MCP server of the session.
    mcp_url: Option<String>,
    /// Placeholder values for `mcp_call` arguments.
    vars: mcp::Vars,
}

struct Fake {
    scenario: Scenario,
    state: Mutex<State>,
    /// `true` once `session/cancel` arrived for the running turn.
    cancelled: watch::Sender<bool>,
}

type Shared = Arc<Fake>;

pub async fn serve(scenario: Scenario) -> Result<(), Error> {
    let state = State {
        mode: scenario.modes.first().cloned().unwrap_or_default(),
        model: scenario.models.first().cloned().unwrap_or_default(),
        effort: "default".into(),
        ..State::default()
    };
    let fake: Shared = Arc::new(Fake {
        scenario,
        state: Mutex::new(state),
        cancelled: watch::Sender::new(false),
    });
    let (f1, f2, f3, f4, f5, f6, f7) = (
        fake.clone(),
        fake.clone(),
        fake.clone(),
        fake.clone(),
        fake.clone(),
        fake.clone(),
        fake,
    );
    Agent
        .builder()
        .name("yhtye-fake-agent")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                gate(&f1, "initialize").await?;
                f1.lock().client_info = req
                    .client_info
                    .as_ref()
                    .map(|c| (c.name.clone(), c.version.clone()));
                let caps = AgentCapabilities::new()
                    .load_session(f1.scenario.load_session)
                    .mcp_capabilities(McpCapabilities::new().http(true));
                responder
                    .respond(InitializeResponse::new(req.protocol_version).agent_capabilities(caps))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: NewSessionRequest, responder, cx| {
                gate(&f2, "session/new").await?;
                enter_session_dir(&req.cwd)?;
                let id = format!("fake-{}", std::process::id());
                f2.remember_session(&id, req.meta.as_ref());
                f2.remember_mcp(&req.mcp_servers);
                send_vendor_notifications(&f2, &cx)?;
                responder.respond(
                    NewSessionResponse::new(id)
                        .modes(f2.modes())
                        .config_options(f2.options()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: LoadSessionRequest, responder, cx| {
                gate(&f3, "session/load").await?;
                enter_session_dir(&req.cwd)?;
                f3.remember_session(&req.session_id.0, req.meta.as_ref());
                f3.remember_mcp(&req.mcp_servers);
                send_vendor_notifications(&f3, &cx)?;
                let replay = format!("replayed history of {}", req.session_id.0);
                send_update(
                    &cx,
                    &req.session_id.0,
                    json!({"sessionUpdate": "user_message_chunk", "content": text(&replay)}),
                )?;
                responder.respond(
                    LoadSessionResponse::new()
                        .modes(f3.modes())
                        .config_options(f3.options()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: SetSessionModeRequest, responder, _cx| {
                gate(&f4, "session/set_mode").await?;
                let mode = req.mode_id.0.to_string();
                if !f4.scenario.modes.contains(&mode) {
                    return responder.respond_with_error(
                        Error::invalid_params().data(json!(format!("unknown mode {mode}"))),
                    );
                }
                f4.lock().mode = mode;
                responder.respond(SetSessionModeResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: SetSessionConfigOptionRequest, responder, _cx| {
                gate(&f5, "session/set_config_option").await?;
                let value = req
                    .value
                    .as_value_id()
                    .map(|v| v.0.to_string())
                    .unwrap_or_default();
                let known = match &*req.config_id.0 {
                    "model" => f5.scenario.models.contains(&value),
                    id if id == f5.effort_id() => f5.effort_values().contains(&value),
                    _ => false,
                };
                if !known {
                    return responder.respond_with_error(
                        Error::invalid_params().data(json!("unknown option/value")),
                    );
                }
                {
                    let mut st = f5.lock();
                    if &*req.config_id.0 == "model" {
                        st.model = value;
                        // The efforts depend on the model: back to the default.
                        st.effort = "default".into();
                    } else {
                        st.effort = value;
                    }
                }
                responder.respond(SetSessionConfigOptionResponse::new(f5.options()))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, cx| {
                let fake = f6.clone();
                let turn_cx = cx.clone();
                // Run the turn off the dispatch loop so session/cancel can arrive.
                cx.spawn(async move {
                    let stop = run_turn(&fake, &turn_cx, &req).await;
                    responder.respond(PromptResponse::new(stop))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            async move |_n: CancelNotification, _cx| {
                f7.cancelled.send_replace(true);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await
}

/// Applies `fail_at` / `exit_at` / `hang_at` for a startup step.
async fn gate(fake: &Fake, step: &str) -> Result<(), Error> {
    let s = &fake.scenario;
    if s.at(&s.exit_at, step) {
        eprintln!("fake: exiting at {step} as scripted");
        std::process::exit(2);
    }
    if s.at(&s.hang_at, step) {
        std::future::pending::<()>().await;
    }
    if s.at(&s.fail_at, step) {
        let error = match s.fail_kind {
            FailKind::Internal => Error::internal_error(),
            FailKind::AuthRequired => Error::auth_required(),
        };
        let data = s
            .fail_message
            .clone()
            .unwrap_or_else(|| format!("fake: scripted failure at {step}"));
        return Err(error.data(json!(data)));
    }
    Ok(())
}

impl Fake {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn remember_session(
        &self,
        id: &str,
        meta: Option<&serde_json::Map<String, serde_json::Value>>,
    ) {
        let mut st = self.lock();
        st.session_id = id.to_string();
        st.meta = meta.cloned();
        st.system_prompt = meta
            .and_then(|m| m.get("systemPrompt"))
            .and_then(|p| p.get("append"))
            .and_then(|a| a.as_str())
            .map(str::to_string);
    }

    fn remember_mcp(&self, servers: &[McpServer]) {
        self.lock().mcp_url = servers.iter().find_map(|s| match s {
            McpServer::Http(h) => Some(h.url.clone()),
            _ => None,
        });
    }

    /// The modes, `None` when the scenario has none (the field is left out).
    fn modes(&self) -> Option<SessionModeState> {
        if self.scenario.modes.is_empty() {
            return None;
        }
        let available = self
            .scenario
            .modes
            .iter()
            .map(|m| SessionMode::new(m.clone(), m.clone()))
            .collect();
        Some(SessionModeState::new(self.lock().mode.clone(), available))
    }

    /// The config options now: the model, plus the effort when the current
    /// model has efforts.
    fn options(&self) -> Vec<SessionConfigOption> {
        let mut options = vec![self.model_option()];
        let values = self.effort_values();
        if !values.is_empty() {
            let rows: Vec<_> = values
                .iter()
                .map(|v| SessionConfigSelectOption::new(v.clone(), v.clone()))
                .collect();
            options.push(
                SessionConfigOption::select(
                    self.effort_id().to_string(),
                    "Effort",
                    self.lock().effort.clone(),
                    rows,
                )
                .category(SessionConfigOptionCategory::ThoughtLevel),
            );
        }
        options
    }

    fn effort_id(&self) -> &str {
        self.scenario
            .effort_id
            .as_deref()
            .unwrap_or(DEFAULT_EFFORT_ID)
    }

    /// `default` and the efforts of the current model (empty: no effort option).
    fn effort_values(&self) -> Vec<String> {
        let model = self.lock().model.clone();
        match self.scenario.efforts.get(&model) {
            Some(values) => std::iter::once("default".to_string())
                .chain(values.iter().cloned())
                .collect(),
            None => Vec::new(),
        }
    }

    fn model_option(&self) -> SessionConfigOption {
        let options: Vec<_> = self
            .scenario
            .models
            .iter()
            .map(|m| SessionConfigSelectOption::new(m.clone(), m.clone()))
            .collect();
        SessionConfigOption::select("model", "Model", self.lock().model.clone(), options)
    }
}

fn text(t: &str) -> serde_json::Value {
    json!({"type": "text", "text": t})
}

fn send_update(
    cx: &ConnectionTo<Client>,
    session: &str,
    update: serde_json::Value,
) -> Result<(), Error> {
    cx.send_notification(RawSessionUpdate(
        json!({"sessionId": session, "update": update}),
    ))
}

/// Sends the scenario's `vendor_notifications` (methods the client does not know).
fn send_vendor_notifications(fake: &Fake, cx: &ConnectionTo<Client>) -> Result<(), Error> {
    for n in &fake.scenario.vendor_notifications {
        cx.send_notification(UntypedMessage::new(&n.method, &n.params)?)?;
    }
    Ok(())
}

fn prompt_text(req: &PromptRequest) -> String {
    req.prompt
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("|")
}

async fn run_turn(fake: &Fake, cx: &ConnectionTo<Client>, req: &PromptRequest) -> StopReason {
    fake.cancelled.send_replace(false);
    let prompt = prompt_text(req);
    let actions = {
        let mut st = fake.lock();
        mcp::capture_prompt_vars(&prompt, &mut st.vars);
        fake.scenario.pick(&prompt, &mut st.next_seq)
    };
    let session = req.session_id.0.to_string();
    if let Err(e) = send_vendor_notifications(fake, cx) {
        eprintln!("fake: action failed: {e}");
        return StopReason::Refusal;
    }
    if fake.scenario.vendor_requests {
        let report = request_diagnostics(cx).await;
        if let Err(e) = send_update(
            cx,
            &session,
            json!({"sessionUpdate": "agent_message_chunk", "content": text(&report)}),
        ) {
            eprintln!("fake: action failed: {e}");
            return StopReason::Refusal;
        }
    }
    for action in actions {
        if *fake.cancelled.borrow() {
            return StopReason::Cancelled;
        }
        match run_action(fake, cx, &session, &prompt, action).await {
            Ok(Some(stop)) => return stop,
            Ok(None) => {}
            Err(e) => {
                eprintln!("fake: action failed: {e}");
                return StopReason::Refusal;
            }
        }
    }
    if *fake.cancelled.borrow() {
        StopReason::Cancelled
    } else {
        StopReason::EndTurn
    }
}

/// Runs one action; `Some(stop)` ends the turn.
async fn run_action(
    fake: &Fake,
    cx: &ConnectionTo<Client>,
    session: &str,
    prompt: &str,
    action: Action,
) -> Result<Option<StopReason>, Error> {
    let update = |u| send_update(cx, session, u);
    match action {
        Action::Message(t) => {
            update(json!({"sessionUpdate": "agent_message_chunk", "content": text(&t)}))?
        }
        Action::Thought(t) => {
            update(json!({"sessionUpdate": "agent_thought_chunk", "content": text(&t)}))?
        }
        Action::ToolCall { id, title } => {
            update(
                json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": title, "kind": "edit", "status": "pending"}),
            )?;
        }
        Action::ToolCallUpdate { id, status } => {
            update(
                json!({"sessionUpdate": "tool_call_update", "toolCallId": id, "status": status}),
            )?;
        }
        Action::Plan(entries) => {
            let entries: Vec<_> = entries
                .iter()
                .map(|e| json!({"content": e, "priority": "medium", "status": "pending"}))
                .collect();
            update(json!({"sessionUpdate": "plan", "entries": entries}))?;
        }
        Action::Update(u) => update(u)?,
        Action::RequestPermission(choices) => {
            let report = request_permission(cx, session, &choices).await?;
            update(json!({"sessionUpdate": "agent_message_chunk", "content": text(&report)}))?;
        }
        Action::AskPermission => {
            let choices = &fake.scenario.permission_options;
            let report = request_permission(cx, session, choices).await?;
            update(json!({"sessionUpdate": "agent_message_chunk", "content": text(&report)}))?;
        }
        Action::Sleep(ms) => {
            if wait_or_cancel(fake, Some(Duration::from_millis(ms))).await {
                return Ok(Some(StopReason::Cancelled));
            }
        }
        Action::WaitCancel => {
            wait_or_cancel(fake, None).await;
            return Ok(Some(StopReason::Cancelled));
        }
        Action::End(reason) => {
            let stop = serde_json::from_value(json!(reason)).unwrap_or(StopReason::EndTurn);
            return Ok(Some(stop));
        }
        Action::Crash(code) => {
            eprintln!("fake: crashing with code {code} as scripted");
            std::process::exit(code);
        }
        Action::SpawnChild => {
            let child = std::process::Command::new("sleep")
                .arg("600")
                .spawn()
                .map_err(Error::into_internal_error)?;
            update(
                json!({"sessionUpdate": "agent_message_chunk", "content": text(&format!("child_pid:{}", child.id()))}),
            )?;
        }
        Action::ReportState => {
            let report = {
                let st = fake.lock();
                let effort = if fake.scenario.efforts.contains_key(&st.model) {
                    format!(";effort={}", st.effort)
                } else {
                    String::new()
                };
                format!(
                    "state:mode={};model={};system_prompt={}{effort};prompt={prompt}",
                    st.mode,
                    st.model,
                    st.system_prompt.as_deref().unwrap_or("<none>")
                )
            };
            update(json!({"sessionUpdate": "agent_message_chunk", "content": text(&report)}))?;
        }
        Action::WriteFile { path, text: body } => {
            std::fs::write(&path, body).map_err(Error::into_internal_error)?;
        }
        Action::Run(argv) => {
            let report = run_command(&argv);
            update(json!({"sessionUpdate": "agent_message_chunk", "content": text(&report)}))?;
        }
        Action::McpCall { tool, args } => {
            let report = mcp_call(fake, &tool, &args).await;
            update(json!({"sessionUpdate": "agent_message_chunk", "content": text(&report)}))?;
        }
        Action::ReportClientInfo => {
            let report = match &fake.lock().client_info {
                Some((name, version)) => format!("client_info:name={name};version={version}"),
                None => "client_info:<none>".into(),
            };
            update(json!({"sessionUpdate": "agent_message_chunk", "content": text(&report)}))?;
        }
        Action::ReportMeta => {
            let meta = serde_json::Value::Object(fake.lock().meta.clone().unwrap_or_default());
            update(
                json!({"sessionUpdate": "agent_message_chunk", "content": text(&format!("meta:{meta}"))}),
            )?;
        }
        Action::McpList => {
            let url = fake.lock().mcp_url.clone();
            let report = match url {
                Some(url) => mcp::list_tools(&url).await.map(|t| t.join(",")),
                None => Err("no HTTP MCP server in session/new".into()),
            };
            let report = format!(
                "mcp:tools:{}",
                report.unwrap_or_else(|e| format!("failed:{e}"))
            );
            update(json!({"sessionUpdate": "agent_message_chunk", "content": text(&report)}))?;
        }
    }
    Ok(None)
}

/// Sends Devin's `_cognition.ai/request_diagnostics` and returns the report
/// line: `vendor:request_diagnostics:ok:<json>` or `:error:<message>`.
async fn request_diagnostics(cx: &ConnectionTo<Client>) -> String {
    let result = cx
        .send_request(RequestDiagnostics(json!({})))
        .block_task()
        .await;
    match result {
        Ok(value) => format!("vendor:request_diagnostics:ok:{value}"),
        Err(e) => format!("vendor:request_diagnostics:error:{e}"),
    }
}

/// Runs one `mcp_call` and returns its report line.
async fn mcp_call(fake: &Fake, tool: &str, args: &serde_json::Value) -> String {
    let (url, args) = {
        let st = fake.lock();
        (st.mcp_url.clone(), mcp::substitute(args, &st.vars))
    };
    let Some(url) = url else {
        return format!("mcp:{tool}:failed:no HTTP MCP server in session/new");
    };
    let result = mcp::call_tool(&url, tool, &args).await;
    if let Ok(r) = &result
        && r.is_error != Some(true)
        && let Some(structured) = &r.structured_content
    {
        mcp::capture_result_vars(structured, &mut fake.lock().vars);
    }
    mcp::report(tool, &result)
}

/// Waits for `timeout` (or forever) unless cancelled first; returns whether cancelled.
async fn wait_or_cancel(fake: &Fake, timeout: Option<Duration>) -> bool {
    let mut rx = fake.cancelled.subscribe();
    let cancelled = rx.wait_for(|c| *c);
    match timeout {
        Some(t) => tokio::time::timeout(t, cancelled).await.is_ok(),
        None => cancelled.await.is_ok(),
    }
}

async fn request_permission(
    cx: &ConnectionTo<Client>,
    session: &str,
    choices: &[PermissionChoice],
) -> Result<String, Error> {
    let options = choices
        .iter()
        .map(|c| {
            let kind: PermissionOptionKind =
                serde_json::from_value(json!(c.kind)).map_err(Error::into_internal_error)?;
            Ok(PermissionOption::new(c.id.clone(), c.id.clone(), kind))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let tool_call = ToolCallUpdate::new(
        "perm-tool",
        ToolCallUpdateFields::new().title("fake tool".to_string()),
    );
    let req = RequestPermissionRequest::new(session.to_string(), tool_call, options);
    let res = cx.send_request(req).block_task().await?;
    Ok(match res.outcome {
        RequestPermissionOutcome::Selected(s) => format!("permission:selected:{}", s.option_id.0),
        _ => "permission:cancelled".into(),
    })
}

/// Works in the session's directory like a real agent (Yhtye starts agent
/// processes elsewhere and passes the working directory only through ACP);
/// `write_file`, `run` and `spawn_child` then act relative to it.
fn enter_session_dir(cwd: &std::path::Path) -> Result<(), Error> {
    std::env::set_current_dir(cwd).map_err(Error::into_internal_error)
}

/// Runs `argv` in the agent's working directory; reports `run:<exit code>`.
fn run_command(argv: &[String]) -> String {
    let Some((program, args)) = argv.split_first() else {
        return "run:error:empty command".into();
    };
    match std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(out) => format!("run:{}", out.status.code().unwrap_or(-1)),
        Err(e) => format!("run:error:{e}"),
    }
}
