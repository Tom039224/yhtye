//! Startup sequence of one agent session:
//! `initialize` → `session/load` or `session/new` → `set_mode` → `set_config_option`.

use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ClientCapabilities, Implementation, InitializeRequest, LoadSessionRequest, McpServer, Meta,
    NewSessionRequest, SessionConfigOption, SessionId, SessionModeState,
    SetSessionConfigOptionRequest, SetSessionModeRequest,
};
use agent_client_protocol::{Agent, ConnectionTo};

use super::config::{HarnessConfig, SystemPromptStyle};
use super::events::{AgentError, AgentInfo, config_value};

/// Everything needed to open the session.
pub(crate) struct StartupParams {
    pub harness: HarnessConfig,
    pub cwd: PathBuf,
    pub mcp: Vec<McpServer>,
    pub resume: Option<SessionId>,
    pub system_prompt: Option<String>,
}

impl StartupParams {
    /// System prompt to prepend to the first prompt (for [`SystemPromptStyle::FirstPrompt`]).
    pub fn first_prompt_preamble(&self, resumed: bool) -> Option<String> {
        match self.harness.system_prompt {
            SystemPromptStyle::FirstPrompt if !resumed => self.system_prompt.clone(),
            _ => None,
        }
    }

    /// `_meta` of `session/new` / `session/load`: the harness' `session_meta` plus
    /// `systemPrompt.append` for [`SystemPromptStyle::MetaAppend`].
    fn session_meta(&self) -> Option<Meta> {
        let mut meta = self.harness.session_meta.clone().unwrap_or_default();
        if let Some(text) = &self.system_prompt
            && self.harness.system_prompt == SystemPromptStyle::MetaAppend
        {
            meta.insert("systemPrompt".into(), serde_json::json!({ "append": text }));
        }
        (!meta.is_empty()).then_some(meta)
    }
}

/// Runs one startup step with the harness timeout, mapping errors to [`AgentError`].
async fn step<T>(
    name: &str,
    timeout: Duration,
    fut: impl Future<Output = Result<T, agent_client_protocol::Error>>,
) -> Result<T, AgentError> {
    match tokio::time::timeout(timeout, fut).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(AgentError::Startup {
            step: name.into(),
            message: e.to_string(),
        }),
        Err(_) => Err(AgentError::Timeout {
            step: name.into(),
            millis: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
        }),
    }
}

/// Opens the session and applies mode / model. Returns the session state.
pub(crate) async fn open_session(
    cx: &ConnectionTo<Agent>,
    p: &StartupParams,
) -> Result<AgentInfo, AgentError> {
    let timeout = p.harness.startup_timeout;
    let init = InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(ClientCapabilities::default())
        .client_info(Implementation::new("yhtye", env!("CARGO_PKG_VERSION")));
    let init = step("initialize", timeout, cx.send_request(init).block_task()).await?;
    let caps = init.agent_capabilities;

    let (session_id, modes, config_options, resumed) = match &p.resume {
        Some(id) if caps.load_session => {
            let req = LoadSessionRequest::new(id.clone(), p.cwd.clone())
                .mcp_servers(p.mcp.clone())
                .meta(p.session_meta());
            let res = step("session/load", timeout, cx.send_request(req).block_task()).await?;
            (id.clone(), res.modes, res.config_options, true)
        }
        resume => {
            if resume.is_some() {
                tracing::warn!("agent does not support session/load; starting a new session");
            }
            let req = NewSessionRequest::new(p.cwd.clone())
                .mcp_servers(p.mcp.clone())
                .meta(p.session_meta());
            let res = step("session/new", timeout, cx.send_request(req).block_task()).await?;
            (res.session_id, res.modes, res.config_options, false)
        }
    };

    let mut info = AgentInfo {
        acp_session_id: session_id,
        resumed,
        modes,
        config_options: config_options.unwrap_or_default(),
        capabilities: caps,
        agent: init.agent_info,
    };
    if let Some(mode) = &p.harness.mode_after_new {
        set_mode(cx, &mut info, mode, timeout).await?;
    }
    if let Some(model) = &p.harness.model {
        info.config_options = set_config_option(
            cx,
            &info.acp_session_id,
            &model.config_id,
            &model.value,
            timeout,
        )
        .await?;
    }
    // The available efforts depend on the model, so the effort always comes
    // after it. A model without that effort makes the start fail
    // (`core-design.md` §15.1).
    if let Some(effort) = &p.harness.effort {
        info.config_options = set_config_option(
            cx,
            &info.acp_session_id,
            &effort.config_id,
            &effort.value,
            timeout,
        )
        .await?;
    }
    Ok(info)
}

/// `session/set_mode`, checked against the advertised modes. Updates `info.modes`.
async fn set_mode(
    cx: &ConnectionTo<Agent>,
    info: &mut AgentInfo,
    mode: &str,
    timeout: Duration,
) -> Result<(), AgentError> {
    if let Some(modes) = &info.modes {
        check_mode_available(modes, mode)?;
    }
    request_set_mode(cx, &info.acp_session_id, mode, timeout).await?;
    if let Some(modes) = &mut info.modes {
        modes.current_mode_id = mode.to_string().into();
    }
    Ok(())
}

/// Sends `session/set_mode`.
pub(crate) async fn request_set_mode(
    cx: &ConnectionTo<Agent>,
    session: &SessionId,
    mode: &str,
    timeout: Duration,
) -> Result<(), AgentError> {
    let req = SetSessionModeRequest::new(session.clone(), mode.to_string());
    step(
        "session/set_mode",
        timeout,
        cx.send_request(req).block_task(),
    )
    .await?;
    Ok(())
}

fn check_mode_available(modes: &SessionModeState, mode: &str) -> Result<(), AgentError> {
    if modes.available_modes.iter().any(|m| &*m.id.0 == mode) {
        return Ok(());
    }
    Err(AgentError::Unsupported {
        requested: format!("mode {mode}"),
        available: modes
            .available_modes
            .iter()
            .map(|m| m.id.0.to_string())
            .collect::<Vec<_>>()
            .join(", "),
    })
}

/// `session/set_config_option`; verifies the agent reports the requested value afterwards.
pub(crate) async fn set_config_option(
    cx: &ConnectionTo<Agent>,
    session: &SessionId,
    config_id: &str,
    value: &str,
    timeout: Duration,
) -> Result<Vec<SessionConfigOption>, AgentError> {
    let req = SetSessionConfigOptionRequest::new(session.clone(), config_id.to_string(), value);
    let res = step(
        "session/set_config_option",
        timeout,
        cx.send_request(req).block_task(),
    )
    .await?;
    match config_value(&res.config_options, config_id) {
        Some(current) if current == value => Ok(res.config_options),
        current => Err(AgentError::Startup {
            step: "session/set_config_option".into(),
            message: format!(
                "requested {config_id}={value} but the agent reports {}",
                current.unwrap_or("<no such option>")
            ),
        }),
    }
}
