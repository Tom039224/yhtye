//! ACP agent-side handlers of the fake agent.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, ContentBlock, InitializeRequest, InitializeResponse,
    LoadSessionRequest, LoadSessionResponse, McpCapabilities, McpServer, NewSessionRequest,
    NewSessionResponse, PermissionOption, PermissionOptionKind, PromptRequest, PromptResponse,
    RequestPermissionOutcome, RequestPermissionRequest, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigSelectGroup, SessionConfigSelectOption,
    SessionConfigSelectOptions, SessionMode, SessionModeState, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, SetSessionModeRequest, SetSessionModeResponse, StopReason,
    ToolCallUpdate, ToolCallUpdateFields,
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Error, Stdio, UntypedMessage};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::watch;

use crate::mcp;
use crate::scenario::{Action, FailKind, PermissionChoice, Scenario};

/// How often `wait_file` looks for its file.
const FILE_POLL: Duration = Duration::from_millis(10);

/// The id of the effort option unless the scenario names another.
const DEFAULT_EFFORT_ID: &str = "effort";

/// The id and category of the option the scenario's `permission_modes` adds.
const PERMISSION_MODE_ID: &str = "permissionMode";
const PERMISSION_MODE_CATEGORY: &str = "_permission";

/// Raw `session/update` so any update JSON can be sent.
#[derive(Debug, Clone, Serialize, Deserialize, agent_client_protocol::JsonRpcNotification)]
#[notification(method = "session/update")]
struct RawSessionUpdate(serde_json::Value);

/// The adapters' `_session/steering` request (see `scenario.rs`).
#[derive(Debug, Clone, Serialize, Deserialize, agent_client_protocol::JsonRpcRequest)]
#[request(method = "_session/steering", response = serde_json::Value)]
#[serde(rename_all = "camelCase")]
struct SteeringRequest {
    session_id: String,
    prompt: Vec<ContentBlock>,
    #[serde(default, rename = "_meta")]
    meta: Option<serde_json::Value>,
}

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
    /// Current value of the `permissionMode` option (when the scenario has one).
    permission_mode: String,
    /// Mode ids of the `session/set_mode` requests received, in order.
    set_modes: Vec<String>,
    /// Config ids of the `session/set_config_option` requests received, in order.
    set_options: Vec<String>,
    system_prompt: Option<String>,
    /// `clientInfo` of `initialize` as `(name, version)`.
    client_info: Option<(String, String)>,
    /// Full `_meta` of `session/new` / `session/load`.
    meta: Option<serde_json::Map<String, serde_json::Value>>,
    next_seq: usize,
    /// A prompt turn is running (set before the turn task starts, so a steer
    /// right behind the prompt finds it).
    turn_running: bool,
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
    /// Text of the steering message injected into the running turn.
    steered: watch::Sender<Option<String>>,
}

type Shared = Arc<Fake>;

pub async fn serve(scenario: Scenario) -> Result<(), Error> {
    let state = State {
        mode: scenario.modes.first().cloned().unwrap_or_default(),
        model: scenario.models.first().cloned().unwrap_or_default(),
        effort: "default".into(),
        permission_mode: scenario
            .permission_modes
            .first()
            .cloned()
            .unwrap_or_default(),
        ..State::default()
    };
    let fake: Shared = Arc::new(Fake {
        scenario,
        state: Mutex::new(state),
        cancelled: watch::Sender::new(false),
        steered: watch::Sender::new(None),
    });
    let (f1, f2, f3, f4, f5, f6, f7, f8) = (
        fake.clone(),
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
                let meta = f1
                    .scenario
                    .steering
                    .then(|| json!({"steering": {"supported": true}}));
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(caps)
                        .meta(meta.and_then(|m| m.as_object().cloned())),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: NewSessionRequest, responder, cx| {
                gate(&f2, "session/new").await?;
                enter_session_dir(&req.cwd)?;
                f2.start_from_env()?;
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
                f3.start_from_env()?;
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
                f4.lock().set_modes.push(mode.clone());
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
                f5.lock().set_options.push(req.config_id.0.to_string());
                let known = match &*req.config_id.0 {
                    "model" => f5.scenario.models.contains(&value),
                    PERMISSION_MODE_ID => f5.scenario.permission_modes.contains(&value),
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
                        // The efforts depend on the model: back to the default.
                        st.effort = f5.default_effort(&value);
                        st.model = value;
                    } else if &*req.config_id.0 == PERMISSION_MODE_ID {
                        st.permission_mode = value;
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
                fake.begin_turn();
                // Run the turn off the dispatch loop so session/cancel can arrive.
                cx.spawn(async move {
                    let stop = run_turn(&fake, &turn_cx, &req).await;
                    responder.respond(PromptResponse::new(stop))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: SteeringRequest, responder, cx| steer(&f8, req, responder, &cx),
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

/// Answers `_session/steering` (see `scenario.rs`); never waits.
fn steer(
    fake: &Shared,
    req: SteeringRequest,
    responder: agent_client_protocol::Responder<serde_json::Value>,
    cx: &ConnectionTo<Client>,
) -> Result<(), Error> {
    if !fake.scenario.steering {
        return responder.respond_with_error(Error::method_not_found());
    }
    let idle_behavior = req
        .meta
        .as_ref()
        .and_then(|m| m.pointer("/steering/idleBehavior"));
    let prompt_required = match idle_behavior {
        None => false,
        Some(b) if b == "promptRequired" => true,
        Some(_) => {
            return responder.respond_with_error(
                Error::invalid_params().data(json!("unsupported steering idleBehavior")),
            );
        }
    };
    let injected = {
        let st = fake.lock();
        if st.turn_running {
            fake.steered.send_replace(Some(blocks_text(&req.prompt)));
        }
        st.turn_running
    };
    if injected {
        return responder.respond(json!({"outcome": "injected"}));
    }
    if prompt_required {
        return responder.respond(json!({"outcome": "promptRequired", "reason": "noRunningTurn"}));
    }
    let prompt = PromptRequest::new(req.session_id, req.prompt);
    let (fake, turn_cx) = (fake.clone(), cx.clone());
    fake.begin_turn();
    cx.spawn(async move {
        run_turn(&fake, &turn_cx, &prompt).await;
        Ok(())
    })?;
    responder.respond(json!({"outcome": "startedNewTurn"}))
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

    /// Marks a turn as running, with no cancel or steer yet.
    fn begin_turn(&self) {
        let mut st = self.lock();
        st.turn_running = true;
        self.cancelled.send_replace(false);
        self.steered.send_replace(None);
    }

    /// Whether the running turn must end: it was cancelled, or steered (unless
    /// a steer lets the turn go on, see `steer_continues`).
    fn interrupted(&self) -> bool {
        *self.cancelled.borrow()
            || (self.steered.borrow().is_some() && !self.scenario.steer_continues)
    }

    /// The text of a steer that the turn goes on after (`steer_continues`).
    fn take_continued_steer(&self) -> Option<String> {
        if self.scenario.steer_continues && !*self.cancelled.borrow() {
            self.steered.send_replace(None)
        } else {
            None
        }
    }

    /// The stop reason of an interrupted turn (a cancel wins over a steer).
    fn interrupted_stop(&self) -> StopReason {
        if *self.cancelled.borrow() {
            StopReason::Cancelled
        } else {
            StopReason::EndTurn
        }
    }

    /// Ends the turn: a steer that arrived (and was not cancelled) is reported
    /// as `steered:<text>` and the turn ends with `end_turn`.
    fn finish_turn(
        &self,
        cx: &ConnectionTo<Client>,
        session: &str,
        stop: StopReason,
    ) -> StopReason {
        let steered = {
            let mut st = self.lock();
            st.turn_running = false;
            self.steered.send_replace(None)
        };
        match steered {
            Some(steer) if stop != StopReason::Cancelled => {
                let report = format!("steered:{steer}");
                if let Err(e) = send_update(
                    cx,
                    session,
                    json!({"sessionUpdate": "agent_message_chunk", "content": text(&report)}),
                ) {
                    eprintln!("fake: action failed: {e}");
                }
                StopReason::EndTurn
            }
            _ => stop,
        }
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
        if self.scenario.modes.is_empty() || self.scenario.modes_as_option {
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

    /// The config options now: the mode when the scenario reports it as an
    /// option, the permission mode when the scenario has one, the model, plus the
    /// effort when the current model has efforts.
    fn options(&self) -> Vec<SessionConfigOption> {
        let mut options = Vec::new();
        if let Some(mode) = self.mode_option() {
            options.push(mode);
        }
        if let Some(permission) = self.permission_mode_option() {
            options.push(permission);
        }
        options.push(self.model_option());
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
        for b in &self.scenario.boolean_options {
            options.push(
                SessionConfigOption::boolean(b.id.clone(), b.id.clone(), true)
                    .category(SessionConfigOptionCategory::Other(b.category.clone())),
            );
        }
        options
    }

    /// The `mode` select option of an agent that reports its mode that way.
    fn mode_option(&self) -> Option<SessionConfigOption> {
        if !self.scenario.modes_as_option || self.scenario.modes.is_empty() {
            return None;
        }
        let rows: Vec<_> = self
            .scenario
            .modes
            .iter()
            .map(|m| SessionConfigSelectOption::new(m.clone(), m.clone()))
            .collect();
        Some(
            SessionConfigOption::select("mode", "Mode", self.lock().mode.clone(), rows)
                .category(SessionConfigOptionCategory::Mode),
        )
    }

    fn permission_mode_option(&self) -> Option<SessionConfigOption> {
        if self.scenario.permission_modes.is_empty() {
            return None;
        }
        let rows: Vec<_> = self
            .scenario
            .permission_modes
            .iter()
            .map(|m| SessionConfigSelectOption::new(m.clone(), m.clone()))
            .collect();
        Some(
            SessionConfigOption::select(
                PERMISSION_MODE_ID,
                "Permission mode",
                self.lock().permission_mode.clone(),
                rows,
            )
            .category(SessionConfigOptionCategory::Other(
                PERMISSION_MODE_CATEGORY.into(),
            )),
        )
    }

    fn effort_id(&self) -> &str {
        self.scenario
            .effort_id
            .as_deref()
            .unwrap_or(DEFAULT_EFFORT_ID)
    }

    /// `default` (unless the scenario has `efforts_without_default`) and the
    /// efforts of the current model (empty: no effort option).
    fn effort_values(&self) -> Vec<String> {
        let model = self.lock().model.clone();
        match self.scenario.efforts.get(&model) {
            Some(values) => (!self.scenario.efforts_without_default)
                .then(|| "default".to_string())
                .into_iter()
                .chain(values.iter().cloned())
                .collect(),
            None => Vec::new(),
        }
    }

    /// The effort a session starts on after `model` is selected: `default`, or
    /// the first of the model's efforts when the scenario lists them without one.
    fn default_effort(&self, model: &str) -> String {
        match self.scenario.efforts.get(model) {
            Some(values) if self.scenario.efforts_without_default => {
                values.first().cloned().unwrap_or_default()
            }
            _ => "default".into(),
        }
    }

    /// Starts the session on the model named by the scenario's `model_env`
    /// variable (`<model>[:<effort>]`). A model the agent does not list is taken
    /// as it is, with no efforts, as pi's adapter does (it builds a model out of
    /// any id of a known provider). No variable, no change.
    fn start_from_env(&self) -> Result<(), Error> {
        let Some(var) = &self.scenario.model_env else {
            return Ok(());
        };
        let Some(wanted) = std::env::var(var).ok().filter(|v| !v.is_empty()) else {
            return Ok(());
        };
        let models = &self.scenario.models;
        let (model, effort) = match wanted.rsplit_once(':') {
            Some((model, effort))
                if !models.contains(&wanted) && models.iter().any(|m| m == model) =>
            {
                (model.to_string(), Some(effort.to_string()))
            }
            _ => (wanted, None),
        };
        let mut st = self.lock();
        let mut start_effort = self.default_effort(&model);
        if let Some(effort) = effort
            && self
                .scenario
                .efforts
                .get(&model)
                .is_some_and(|values| values.contains(&effort))
        {
            start_effort = effort;
        }
        st.effort = start_effort;
        st.model = model;
        Ok(())
    }

    fn model_option(&self) -> SessionConfigOption {
        let rows = |models: Vec<&String>| -> Vec<SessionConfigSelectOption> {
            models
                .into_iter()
                .map(|m| SessionConfigSelectOption::new(m.clone(), m.clone()))
                .collect()
        };
        let current = self.lock().model.clone();
        if !self.scenario.model_groups {
            let options = rows(self.scenario.models.iter().collect());
            return SessionConfigOption::select("model", "Model", current, options);
        }
        let mut providers: Vec<&str> = Vec::new();
        for m in &self.scenario.models {
            let provider = m.split('/').next().unwrap_or(m);
            if !providers.contains(&provider) {
                providers.push(provider);
            }
        }
        let groups: Vec<_> = providers
            .into_iter()
            .map(|provider| {
                let members = self
                    .scenario
                    .models
                    .iter()
                    .filter(|m| m.split('/').next() == Some(provider))
                    .collect();
                SessionConfigSelectGroup::new(
                    provider.to_string(),
                    provider.to_string(),
                    rows(members),
                )
            })
            .collect();
        SessionConfigOption::select(
            "model",
            "Model",
            current,
            SessionConfigSelectOptions::Grouped(groups),
        )
        .category(SessionConfigOptionCategory::Model)
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
    blocks_text(&req.prompt)
}

fn blocks_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Runs a turn begun with [`Fake::begin_turn`].
async fn run_turn(fake: &Fake, cx: &ConnectionTo<Client>, req: &PromptRequest) -> StopReason {
    let stop = run_turn_actions(fake, cx, req).await;
    fake.finish_turn(cx, &req.session_id.0, stop)
}

async fn run_turn_actions(
    fake: &Fake,
    cx: &ConnectionTo<Client>,
    req: &PromptRequest,
) -> StopReason {
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
        if fake.interrupted() {
            return fake.interrupted_stop();
        }
        if let Some(steer) = fake.take_continued_steer() {
            let report = format!("steered:{steer}");
            let update = json!({"sessionUpdate": "agent_message_chunk", "content": text(&report)});
            if let Err(e) = send_update(cx, &session, update) {
                eprintln!("fake: action failed: {e}");
                return StopReason::Refusal;
            }
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
    fake.interrupted_stop()
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
            if wait_or_interrupt(fake, Some(Duration::from_millis(ms))).await && fake.interrupted()
            {
                return Ok(Some(fake.interrupted_stop()));
            }
        }
        Action::Busy(ms) => tokio::time::sleep(Duration::from_millis(ms)).await,
        Action::WaitCancel => {
            wait_or_interrupt(fake, None).await;
            return Ok(Some(fake.interrupted_stop()));
        }
        Action::WaitFile(path) => {
            while !std::path::Path::new(&path).exists() {
                if wait_or_interrupt(fake, Some(FILE_POLL)).await && fake.interrupted() {
                    return Ok(Some(fake.interrupted_stop()));
                }
            }
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
        Action::ReportWrites => {
            let report = {
                let st = fake.lock();
                format!(
                    "writes:modes={};options={}",
                    st.set_modes.join(","),
                    st.set_options.join(",")
                )
            };
            update(json!({"sessionUpdate": "agent_message_chunk", "content": text(&report)}))?;
        }
        Action::ReportEnv(names) => {
            let values: Vec<String> = names
                .iter()
                .map(|n| {
                    let value = std::env::var(n).unwrap_or_else(|_| "<unset>".into());
                    format!("{n}={value}")
                })
                .collect();
            let report = format!("env:{}", values.join(";"));
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

/// Waits for `timeout` (or forever) unless cancelled or steered first;
/// returns whether interrupted.
async fn wait_or_interrupt(fake: &Fake, timeout: Option<Duration>) -> bool {
    let (mut cancel_rx, mut steer_rx) = (fake.cancelled.subscribe(), fake.steered.subscribe());
    let interrupted = async {
        tokio::select! {
            _ = cancel_rx.wait_for(|c| *c) => {}
            _ = steer_rx.wait_for(Option::is_some) => {}
        }
    };
    match timeout {
        Some(t) => tokio::time::timeout(t, interrupted).await.is_ok(),
        None => {
            interrupted.await;
            true
        }
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
