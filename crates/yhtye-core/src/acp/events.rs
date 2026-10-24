//! Typed events emitted by an agent session actor.

use agent_client_protocol::schema::v1::{
    AgentCapabilities, AvailableCommandsUpdate, ConfigOptionUpdate, ContentChunk,
    CurrentModeUpdate, Implementation, PermissionOption, PermissionOptionKind, Plan,
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory, SessionId,
    SessionInfoUpdate, SessionModeState, SessionUpdate, StopReason, ToolCall, ToolCallUpdate,
    UsageUpdate,
};
use schemars::JsonSchema;
use serde::Serialize;
use ts_rs::TS;

/// Everything an agent session reports, in order.
///
/// Ordering guarantees: `Ready` (or a startup error from `spawn_agent`) comes first;
/// the `Output`s of a turn precede its `TurnEnded`; `Exited` is always the last event.
// Events are moved through a channel once; boxing every `Output` would cost more
// than the size difference.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, JsonSchema, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum AgentEvent {
    Ready(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        Box<AgentInfo>,
    ),
    /// One `session/update`, typed 1:1 (unknown kinds become [`AgentOutput::Unknown`]).
    Output(AgentOutput),
    PermissionAutoAnswered {
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        tool_call: Box<ToolCallUpdate>,
        #[ts(type = "Array<unknown>")]
        #[schemars(with = "Vec<serde_json::Value>")]
        options: Vec<PermissionOption>,
        /// `None` means the request was answered `Cancelled` (no allow option).
        #[ts(type = "string | null")]
        #[schemars(with = "Option<String>")]
        chosen: Option<PermissionOptionKind>,
    },
    TurnEnded(
        #[ts(as = "Result<String, AgentError>")]
        #[schemars(with = "Result<String, AgentError>")]
        Result<StopReason, AgentError>,
    ),
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
    /// The agent advertises `_session/steering` (top-level `_meta.steering.supported`
    /// of the `initialize` result); see [`super::AgentHandle::steer`].
    pub supports_steering: bool,
    /// How long each startup step took, in the order they ran.
    pub startup_steps: Vec<StartupStep>,
}

/// How long one startup step took: the request the client sent to open the
/// session (`initialize`, `session/new` or `session/load`, `session/set_mode`,
/// `session/set_config_option`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct StartupStep {
    pub step: String,
    /// Milliseconds from sending the request to its answer, its error or its
    /// timeout (a step that timed out took the whole timeout).
    pub millis: u64,
}

impl std::fmt::Display for StartupStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} ms", self.step, self.millis)
    }
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

/// The select option holding the effort (thought level): the one with id
/// `config_id`, else the select of category `thought_level` when there is
/// exactly one (an agent may name its effort option anything, e.g. Devin; with
/// several candidates none is guessed).
#[must_use]
pub fn effort_option<'a>(
    options: &'a [SessionConfigOption],
    config_id: &str,
) -> Option<&'a SessionConfigOption> {
    let selects = options
        .iter()
        .filter(|o| matches!(o.kind, SessionConfigKind::Select(_)));
    if let Some(named) = selects.clone().find(|o| &*o.id.0 == config_id) {
        return Some(named);
    }
    let mut thought_levels =
        selects.filter(|o| o.category == Some(SessionConfigOptionCategory::ThoughtLevel));
    match (thought_levels.next(), thought_levels.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// A `session/update`, typed. Mirrors `SessionUpdate` of the ACP schema.
#[derive(Debug, Clone, Serialize, JsonSchema, TS)]
#[serde(tag = "kind", content = "update", rename_all = "snake_case")]
pub enum AgentOutput {
    UserMessageChunk(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        ContentChunk,
    ),
    MessageChunk(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        ContentChunk,
    ),
    ThoughtChunk(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        ContentChunk,
    ),
    ToolCall(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        Box<ToolCall>,
    ),
    ToolCallUpdate(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        Box<ToolCallUpdate>,
    ),
    Plan(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        Plan,
    ),
    AvailableCommands(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        AvailableCommandsUpdate,
    ),
    ModeChanged(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        CurrentModeUpdate,
    ),
    ConfigOptions(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        ConfigOptionUpdate,
    ),
    SessionInfo(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        SessionInfoUpdate,
    ),
    Usage(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        UsageUpdate,
    ),
    /// An update kind this build does not know (kept verbatim, never dropped).
    Unknown(
        #[ts(type = "unknown")]
        #[schemars(with = "serde_json::Value")]
        serde_json::Value,
    ),
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

/// Heading of the agent's stderr tail inside an error text.
const STDERR_TAIL_HEADING: &str = "--- agent stderr (tail) ---";

/// The stderr `tail` as a section to append to an error text (empty when the
/// agent wrote nothing): a newline, [`STDERR_TAIL_HEADING`], the lines.
pub(crate) fn stderr_section(tail: Option<&str>) -> String {
    match tail {
        Some(tail) if !tail.is_empty() => format!("\n{STDERR_TAIL_HEADING}\n{tail}"),
        _ => String::new(),
    }
}

/// Splits an error text into the message and its stderr section
/// ([`stderr_section`] without the leading newline), if it has one.
fn split_stderr_tail(text: &str) -> (&str, Option<&str>) {
    match text.find(&format!("\n{STDERR_TAIL_HEADING}\n")) {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    }
}

/// `sentence(message)` for an error text, with the agent's stderr section (if
/// the text has one) after the sentence instead of inside it.
pub(crate) fn sentence_around_message(text: &str, sentence: impl FnOnce(&str) -> String) -> String {
    let (message, tail) = split_stderr_tail(text);
    let mut sentence = sentence(message);
    if let Some(tail) = tail {
        sentence.push('\n');
        sentence.push_str(tail);
    }
    sentence
}

/// Errors surfaced by the ACP client. Messages are meant to be shown to people.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, JsonSchema, TS)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum AgentError {
    #[error("failed to launch agent `{command}`: {message}")]
    Spawn { command: String, message: String },
    /// `message` ends with the agent's stderr tail when it wrote any.
    #[error("agent startup failed at `{step}`: {message}")]
    Startup {
        step: String,
        message: String,
        /// The startup steps that ran, the failed one last (absent when unknown).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        steps: Option<Vec<StartupStep>>,
    },
    #[error(
        "agent startup timed out at `{step}` after {millis} ms{}",
        stderr_section(.stderr.as_deref())
    )]
    Timeout {
        step: String,
        millis: u64,
        /// The tail of the agent's stderr when it wrote any (a hung agent
        /// usually says why there), shown with the error.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        stderr: Option<String>,
        /// The startup steps that ran, the timed out one last (absent when unknown).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        steps: Option<Vec<StartupStep>>,
    },
    #[error("`{requested}` is not offered by the agent (available: {available})")]
    Unsupported {
        requested: String,
        available: String,
    },
    /// The agent cannot be chosen (no harness is installed, a harness has no
    /// model): the message says what to do.
    #[error("{message}")]
    Setup { message: String },
    #[error("a turn is already running on this session")]
    Busy,
    /// The agent does not advertise `_session/steering`; nothing was sent.
    #[error("the agent does not support steering a running turn")]
    SteeringUnsupported,
    #[error("agent request `{method}` failed: {message}")]
    Request { method: String, message: String },
    #[error("the agent session is closed")]
    Closed,
}

impl AgentError {
    /// A startup failure at `step`, with no step timings.
    pub(crate) fn startup(step: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Startup {
            step: step.into(),
            message: message.into(),
            steps: None,
        }
    }

    /// A startup timeout at `step` (no stderr tail, no step timings yet).
    pub(crate) fn timeout(step: impl Into<String>, millis: u64) -> Self {
        Self::Timeout {
            step: step.into(),
            millis,
            stderr: None,
            steps: None,
        }
    }

    /// The startup steps that ran before this error, the failed one last
    /// (empty for errors that are not startup failures, or when not known).
    #[must_use]
    pub fn startup_steps(&self) -> &[StartupStep] {
        match self {
            Self::Startup { steps, .. } | Self::Timeout { steps, .. } => {
                steps.as_deref().unwrap_or_default()
            }
            _ => &[],
        }
    }

    /// This error with `startup_steps` recorded, if it is a startup failure.
    #[must_use]
    pub(crate) fn with_startup_steps(self, startup_steps: Vec<StartupStep>) -> Self {
        let startup_steps = Some(startup_steps).filter(|steps| !steps.is_empty());
        match self {
            Self::Startup { step, message, .. } => Self::Startup {
                step,
                message,
                steps: startup_steps,
            },
            Self::Timeout {
                step,
                millis,
                stderr,
                ..
            } => Self::Timeout {
                step,
                millis,
                stderr,
                steps: startup_steps,
            },
            other => other,
        }
    }
}

/// `steps` as one line for logs: `initialize 812 ms, session/new 3411 ms`.
#[must_use]
pub fn format_startup_steps(steps: &[StartupStep]) -> String {
    steps
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timeout_shows_the_stderr_tail_only_when_there_is_one() {
        let plain = AgentError::timeout("session/set_mode", 120_000);
        assert_eq!(
            plain.to_string(),
            "agent startup timed out at `session/set_mode` after 120000 ms"
        );
        let AgentError::Timeout {
            step,
            millis,
            steps,
            ..
        } = plain
        else {
            unreachable!()
        };
        let silent = AgentError::Timeout {
            step,
            millis,
            stderr: Some(String::new()),
            steps,
        };
        assert_eq!(
            silent.to_string(),
            "agent startup timed out at `session/set_mode` after 120000 ms",
            "an empty tail adds nothing"
        );

        let said = AgentError::Timeout {
            step: "session/set_mode".into(),
            millis: 120_000,
            stderr: Some("connect ETIMEDOUT\nretrying".into()),
            steps: None,
        };
        assert_eq!(
            said.to_string(),
            "agent startup timed out at `session/set_mode` after 120000 ms\n\
             --- agent stderr (tail) ---\nconnect ETIMEDOUT\nretrying"
        );
    }

    #[test]
    fn the_stderr_section_splits_off_an_error_text() {
        let text = format!("agent startup failed{}", stderr_section(Some("a\nb")));
        assert_eq!(
            split_stderr_tail(&text),
            (
                "agent startup failed",
                Some("--- agent stderr (tail) ---\na\nb")
            )
        );
        assert_eq!(split_stderr_tail("no tail"), ("no tail", None));
        assert_eq!(
            sentence_around_message(&text, |m| format!("It failed: {m}. Try again.")),
            "It failed: agent startup failed. Try again.\n--- agent stderr (tail) ---\na\nb"
        );
        assert_eq!(
            sentence_around_message("no tail", |m| format!("It failed: {m}. Try again.")),
            "It failed: no tail. Try again."
        );
        assert_eq!(stderr_section(None), "");
        assert_eq!(stderr_section(Some("")), "");
    }

    #[test]
    fn startup_steps_are_kept_only_on_startup_failures() {
        let steps = vec![
            StartupStep {
                step: "initialize".into(),
                millis: 812,
            },
            StartupStep {
                step: "session/new".into(),
                millis: 3411,
            },
        ];
        let timeout = AgentError::timeout("session/new", 3411).with_startup_steps(steps.clone());
        assert_eq!(timeout.startup_steps(), steps);
        let startup = AgentError::startup("session/new", "bad").with_startup_steps(steps.clone());
        assert_eq!(startup.startup_steps(), steps);
        assert!(
            AgentError::Closed
                .with_startup_steps(steps.clone())
                .startup_steps()
                .is_empty()
        );
        assert_eq!(
            format_startup_steps(&steps),
            "initialize 812 ms, session/new 3411 ms"
        );
    }

    #[test]
    fn the_optional_fields_of_the_startup_errors_are_left_out_when_empty() {
        // The wire shape stays the one older clients read.
        let value = serde_json::to_value(AgentError::timeout("initialize", 500)).expect("json");
        assert_eq!(
            value,
            serde_json::json!({"code": "timeout", "step": "initialize", "millis": 500})
        );
        let value = serde_json::to_value(AgentError::startup("initialize", "x")).expect("json");
        assert_eq!(
            value,
            serde_json::json!({"code": "startup", "step": "initialize", "message": "x"})
        );
    }
}
