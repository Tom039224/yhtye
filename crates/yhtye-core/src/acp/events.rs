//! Typed events emitted by an agent session actor.

use agent_client_protocol::schema::v1::{
    AgentCapabilities, AvailableCommandsUpdate, ConfigOptionUpdate, ContentChunk,
    CurrentModeUpdate, Implementation, PermissionOption, PermissionOptionKind, Plan,
    SessionConfigKind, SessionConfigOption, SessionId, SessionInfoUpdate, SessionModeState,
    SessionUpdate, StopReason, ToolCall, ToolCallUpdate, UsageUpdate,
};
use serde::Serialize;
use ts_rs::TS;

/// Everything an agent session reports, in order.
///
/// Ordering guarantees: `Ready` (or a startup error from `spawn_agent`) comes first;
/// the `Output`s of a turn precede its `TurnEnded`; `Exited` is always the last event.
// Events are moved through a channel once; boxing every `Output` would cost more
// than the size difference.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum AgentEvent {
    Ready(#[ts(type = "unknown")] Box<AgentInfo>),
    /// One `session/update`, typed 1:1 (unknown kinds become [`AgentOutput::Unknown`]).
    Output(AgentOutput),
    PermissionAutoAnswered {
        #[ts(type = "unknown")]
        tool_call: Box<ToolCallUpdate>,
        #[ts(type = "Array<unknown>")]
        options: Vec<PermissionOption>,
        /// `None` means the request was answered `Cancelled` (no allow option).
        #[ts(type = "string | null")]
        chosen: Option<PermissionOptionKind>,
    },
    TurnEnded(#[ts(as = "Result<String, AgentError>")] Result<StopReason, AgentError>),
    /// One stderr line of the agent process (for logs, not for the UI).
    Stderr(String),
    /// The agent process is gone (always the final event).
    Exited {
        code: Option<i32>,
        signal: Option<i32>,
    },
}

/// Session state after a successful startup (also returned in [`super::AgentHandle`]).
#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub acp_session_id: SessionId,
    /// True when the session was restored with `session/load`.
    pub resumed: bool,
    pub modes: Option<SessionModeState>,
    pub config_options: Vec<SessionConfigOption>,
    pub capabilities: AgentCapabilities,
    pub agent: Option<Implementation>,
}

impl AgentInfo {
    /// Current value of a select-type config option (e.g. `"model"`).
    #[must_use]
    pub fn config_value(&self, config_id: &str) -> Option<&str> {
        config_value(&self.config_options, config_id)
    }

    /// Current session mode id: the `modes` state, or else the `mode` config
    /// option (OpenCode reports its modes only as a config option).
    #[must_use]
    pub fn current_mode(&self) -> Option<&str> {
        self.modes
            .as_ref()
            .map(|m| &*m.current_mode_id.0)
            .or_else(|| self.config_value("mode"))
    }
}

/// Current value of a select-type config option in `options`.
#[must_use]
pub fn config_value<'a>(options: &'a [SessionConfigOption], config_id: &str) -> Option<&'a str> {
    options
        .iter()
        .find(|o| &*o.id.0 == config_id)
        .and_then(|o| match &o.kind {
            SessionConfigKind::Select(s) => Some(&*s.current_value.0),
            _ => None,
        })
}

/// A `session/update`, typed. Mirrors `SessionUpdate` of the ACP schema.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "kind", content = "update", rename_all = "snake_case")]
pub enum AgentOutput {
    UserMessageChunk(#[ts(type = "unknown")] ContentChunk),
    MessageChunk(#[ts(type = "unknown")] ContentChunk),
    ThoughtChunk(#[ts(type = "unknown")] ContentChunk),
    ToolCall(#[ts(type = "unknown")] Box<ToolCall>),
    ToolCallUpdate(#[ts(type = "unknown")] Box<ToolCallUpdate>),
    Plan(#[ts(type = "unknown")] Plan),
    AvailableCommands(#[ts(type = "unknown")] AvailableCommandsUpdate),
    ModeChanged(#[ts(type = "unknown")] CurrentModeUpdate),
    ConfigOptions(#[ts(type = "unknown")] ConfigOptionUpdate),
    SessionInfo(#[ts(type = "unknown")] SessionInfoUpdate),
    Usage(#[ts(type = "unknown")] UsageUpdate),
    /// An update kind this build does not know (kept verbatim, never dropped).
    Unknown(#[ts(type = "unknown")] serde_json::Value),
}

impl AgentOutput {
    /// Types the `update` field of a raw `session/update` notification.
    #[must_use]
    pub fn from_update_json(update: serde_json::Value) -> Self {
        match serde_json::from_value::<SessionUpdate>(update.clone()) {
            Ok(typed) => Self::from_update(typed, update),
            Err(_) => Self::Unknown(update),
        }
    }

    fn from_update(update: SessionUpdate, raw: serde_json::Value) -> Self {
        match update {
            SessionUpdate::UserMessageChunk(c) => Self::UserMessageChunk(c),
            SessionUpdate::AgentMessageChunk(c) => Self::MessageChunk(c),
            SessionUpdate::AgentThoughtChunk(c) => Self::ThoughtChunk(c),
            SessionUpdate::ToolCall(t) => Self::ToolCall(Box::new(t)),
            SessionUpdate::ToolCallUpdate(t) => Self::ToolCallUpdate(Box::new(t)),
            SessionUpdate::Plan(p) => Self::Plan(p),
            SessionUpdate::AvailableCommandsUpdate(u) => Self::AvailableCommands(u),
            SessionUpdate::CurrentModeUpdate(u) => Self::ModeChanged(u),
            SessionUpdate::ConfigOptionUpdate(u) => Self::ConfigOptions(u),
            SessionUpdate::SessionInfoUpdate(u) => Self::SessionInfo(u),
            SessionUpdate::UsageUpdate(u) => Self::Usage(u),
            _ => Self::Unknown(raw),
        }
    }

    /// Text of a message / thought chunk, if this is one with text content.
    #[must_use]
    pub fn chunk_text(&self) -> Option<&str> {
        use agent_client_protocol::schema::v1::ContentBlock;
        match self {
            Self::UserMessageChunk(c) | Self::MessageChunk(c) | Self::ThoughtChunk(c) => {
                match &c.content {
                    ContentBlock::Text(t) => Some(&t.text),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// Errors surfaced by the ACP client. Messages are meant to be shown to people.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, TS)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum AgentError {
    #[error("failed to launch agent `{command}`: {message}")]
    Spawn { command: String, message: String },
    #[error("agent startup failed at `{step}`: {message}")]
    Startup { step: String, message: String },
    #[error("agent startup timed out at `{step}` after {millis} ms")]
    Timeout { step: String, millis: u64 },
    #[error("`{requested}` is not offered by the agent (available: {available})")]
    Unsupported {
        requested: String,
        available: String,
    },
    #[error("a turn is already running on this session")]
    Busy,
    #[error("agent request `{method}` failed: {message}")]
    Request { method: String, message: String },
    #[error("the agent session is closed")]
    Closed,
}
