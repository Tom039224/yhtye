//! Harness configuration. All harness-specific knowledge (launch command, env,
//! mode names, how to pick a model) lives here; the rest of the ACP client is generic.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Default timeout for each startup step (initialize, session/new, ...).
/// Generous because `npx -y` may download the adapter on first use.
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);

/// How to launch and configure one ACP harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessConfig {
    /// Executable (resolved through `PATH`).
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment for the agent process (inherits the parent env otherwise).
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// `session/set_mode` sent right after `session/new` / `session/load`.
    #[serde(default)]
    pub mode_after_new: Option<String>,
    /// `session/set_config_option` sent after the mode is set.
    #[serde(default)]
    pub model: Option<ModelSelect>,
    #[serde(default)]
    pub system_prompt: SystemPromptStyle,
    /// Timeout applied to each startup step.
    #[serde(default = "default_startup_timeout", with = "duration_secs")]
    pub startup_timeout: Duration,
}

/// Selects the model via `session/set_config_option`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSelect {
    pub config_id: String,
    pub value: String,
}

/// How the role system prompt reaches the agent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemPromptStyle {
    /// `session/new` `_meta.systemPrompt.append` (Claude Code adapter).
    #[default]
    MetaAppend,
    /// Prepended as a text block to the first prompt of the session.
    FirstPrompt,
}

impl HarnessConfig {
    /// Claude Code through `@agentclientprotocol/claude-agent-acp` with the given model
    /// alias (see `docs/architecture/acp-harnesses.md` §5). The model is set both through
    /// `ANTHROPIC_MODEL` (initial value) and `set_config_option` (verifiable).
    #[must_use]
    pub fn claude_code(model: &str) -> Self {
        Self {
            command: "npx".into(),
            args: vec![
                "-y".into(),
                "@agentclientprotocol/claude-agent-acp@0.81".into(),
            ],
            env: BTreeMap::from([("ANTHROPIC_MODEL".into(), model.into())]),
            mode_after_new: Some("bypassPermissions".into()),
            model: Some(ModelSelect {
                config_id: "model".into(),
                value: model.into(),
            }),
            system_prompt: SystemPromptStyle::MetaAppend,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
        }
    }

    /// A bare harness running `command args...` with no mode/model configuration.
    #[must_use]
    pub fn plain(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            command: command.into(),
            args,
            env: BTreeMap::new(),
            mode_after_new: None,
            model: None,
            system_prompt: SystemPromptStyle::MetaAppend,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
        }
    }
}

fn default_startup_timeout() -> Duration {
    DEFAULT_STARTUP_TIMEOUT
}

mod duration_secs {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(d.as_secs_f64())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let secs = f64::deserialize(d)?;
        Duration::try_from_secs_f64(secs).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_defaults_use_bypass_and_requested_model() {
        let h = HarnessConfig::claude_code("haiku");
        assert_eq!(h.mode_after_new.as_deref(), Some("bypassPermissions"));
        assert_eq!(
            h.env.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("haiku")
        );
        let model = h.model.expect("model select");
        assert_eq!(
            (model.config_id.as_str(), model.value.as_str()),
            ("model", "haiku")
        );
        assert_eq!(h.system_prompt, SystemPromptStyle::MetaAppend);
    }

    #[test]
    fn deserializes_with_defaults() {
        let h: HarnessConfig = serde_json::from_str(r#"{"command":"x"}"#).expect("parse");
        assert_eq!(h, HarnessConfig::plain("x", vec![]));
        let h: HarnessConfig = serde_json::from_str(
            r#"{"command":"x","startup_timeout":1.5,"system_prompt":"first_prompt"}"#,
        )
        .expect("parse");
        assert_eq!(h.startup_timeout, Duration::from_millis(1500));
        assert_eq!(h.system_prompt, SystemPromptStyle::FirstPrompt);
    }
}
