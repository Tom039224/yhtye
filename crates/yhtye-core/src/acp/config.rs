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
    /// Inherited environment variables removed for the agent process (Stage 7c-2:
    /// an `OPENCODE_CONFIG_DIR` injected by the terminal Yhtye was started from).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_remove: Vec<String>,
    /// `session/set_mode` sent right after `session/new` / `session/load`.
    #[serde(default)]
    pub mode_after_new: Option<String>,
    /// `session/set_config_option` sent after the mode is set.
    #[serde(default)]
    pub model: Option<ModelSelect>,
    /// `session/set_config_option` for the effort (thought level), sent right
    /// after the model: the available efforts depend on the model (Stage 7d).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ModelSelect>,
    #[serde(default)]
    pub system_prompt: SystemPromptStyle,
    /// Extra harness-specific fields merged into `_meta` of `session/new` /
    /// `session/load` (a JSON object; `systemPrompt` is added by Yhtye).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_meta: Option<serde_json::Map<String, serde_json::Value>>,
    /// Timeout applied to each startup step.
    #[serde(default = "default_startup_timeout", with = "duration_secs")]
    pub startup_timeout: Duration,
    /// `clientInfo` sent in `initialize` instead of Yhtye's own, for a harness
    /// that only behaves for clients it knows. `None` (the default) introduces
    /// Yhtye as `yhtye`: another product's name is never claimed on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_info: Option<ClientInfoOverride>,
    /// How `session/request_permission` is answered automatically.
    #[serde(default, skip_serializing_if = "PermissionPolicy::is_default")]
    pub permission_policy: PermissionPolicy,
    /// Harness-specific text appended to the role system prompt (however it is
    /// delivered), for what the role prompt cannot say for every harness (Devin
    /// reaches MCP tools only through its own meta-tools). Nothing is sent when
    /// the session has no role prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_note: Option<String>,
}

/// The Claude Code ACP adapter run through `npx` (an exact version).
pub const CLAUDE_AGENT_ACP: &str = "@agentclientprotocol/claude-agent-acp@0.84.0";

/// OpenCode's built-in primary agent ("mode") with every tool allowed except
/// asking for paths outside the session directory (answered by Yhtye).
pub const OPENCODE_BUILD_MODE: &str = "build";

/// The default config id of the effort option (Claude Code's `thought_level`,
/// OpenCode's variant). A preset may name another one (Codex: `reasoning_effort`).
pub const EFFORT_CONFIG_ID: &str = "effort";

/// The Codex ACP adapter run through `npx` (an exact version, like
/// [`CLAUDE_AGENT_ACP`]; `docs/architecture/acp-harnesses.md` §9).
pub const CODEX_ACP: &str = "@agentclientprotocol/codex-acp@2.0.0";

/// Codex's mode without approvals and without a sandbox (the "yolo" of
/// Claude Code's `bypassPermissions` / OpenCode's `build`).
pub const CODEX_FULL_ACCESS_MODE: &str = "agent-full-access";

/// The adapter's environment variable with session settings (a JSON object
/// merged into Codex's thread config: `model`, `model_reasoning_effort`, ...).
pub const CODEX_CONFIG_ENV: &str = "CODEX_CONFIG";

/// The adapter's environment variable naming the `codex` executable to use.
pub const CODEX_PATH_ENV: &str = "CODEX_PATH";

/// Devin's built-in mode without permission prompts (`devin acp` ignores
/// `--permission-mode`, so the mode is switched with `session/set_mode`).
pub const DEVIN_BYPASS_MODE: &str = "bypass";

/// Appended to every role prompt of Devin ([`HarnessConfig::system_prompt_note`]):
/// Devin does not list MCP tools among its own tools, it reaches them through
/// its MCP meta-tools (`mcp_list_tools` was seen with Devin 3000.11.3), so the
/// tool names of the role prompts (`mcp__yhtye__report_step_done`) are not
/// found as such (`docs/architecture/acp-harnesses.md` §10.7).
pub const DEVIN_MCP_NOTE: &str = "\
Note for Devin: Yhtye's tools (`report_step_done`, `help`, `create_group`, ...) \
are on the MCP server named `yhtye`; they are not among your built-in tools. \
List them with your MCP tool-listing tool (`mcp_list_tools`, server `yhtye`) \
and call them with your MCP tool-calling tool. A name like \
`mcp__yhtye__report_step_done` means the tool `report_step_done` of the MCP \
server `yhtye`.";

/// Inherited variables removed from Devin's environment: Devin logs with
/// `tracing` too, so a `RUST_LOG` meant for Yhtye would filter Devin's own log
/// (`~/.local/share/devin/cli/logs`) down to nothing.
pub const DEVIN_ENV_REMOVE: [&str; 1] = ["RUST_LOG"];

/// Selects a value (the model, the effort) via `session/set_config_option`.
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

/// The `clientInfo` of `initialize` (see [`HarnessConfig::client_info`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfoOverride {
    pub name: String,
    pub version: String,
}

/// How permission requests are answered without asking anyone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicy {
    /// `allow_always` if offered, else `allow_once` (Claude Code, OpenCode, Codex).
    #[default]
    Default,
    /// Only `allow_once`, and never an option that switches the mode / plan or
    /// grants a standing permission (Devin offers `switch_*`, `plan_*` and
    /// `*_always` options that would change how the session runs).
    OnceOnly,
}

impl PermissionPolicy {
    fn is_default(&self) -> bool {
        *self == Self::Default
    }
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
                // Exact version: `npx -y` runs whatever it resolves, so a range
                // would pull and execute new releases automatically.
                CLAUDE_AGENT_ACP.into(),
            ],
            env: BTreeMap::from([
                ("ANTHROPIC_MODEL".into(), model.into()),
                // Keep MCP tools (mcp__yhtye__*) loaded instead of deferring them
                // behind ToolSearch, and do not attach the user's claude.ai connectors.
                ("ENABLE_TOOL_SEARCH".into(), "false".into()),
                ("ENABLE_CLAUDEAI_MCP_SERVERS".into(), "false".into()),
                // No background shell commands / tasks: a turn that ends while a
                // command runs in the background can never report (nothing wakes
                // the session up again), which Yhtye counts as a silent turn.
                ("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS".into(), "1".into()),
            ]),
            env_remove: Vec::new(),
            mode_after_new: Some("bypassPermissions".into()),
            model: Some(ModelSelect {
                config_id: "model".into(),
                value: model.into(),
            }),
            effort: None,
            system_prompt: SystemPromptStyle::MetaAppend,
            // Only the MCP servers Yhtye passes (not the user's .mcp.json / plugins).
            session_meta:
                serde_json::json!({ "claudeCode": { "options": { "strictMcpConfig": true } } })
                    .as_object()
                    .cloned(),
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            client_info: None,
            permission_policy: PermissionPolicy::Default,
            system_prompt_note: None,
        }
    }

    /// [`HarnessConfig::claude_code`] for reading subscription usage with the
    /// local `/usage` command (`crate::usage`): no tools, no MCP servers, the
    /// session is not saved to `~/.claude/projects` (`persistSession: false`),
    /// no mode / model switching (the command never reaches the model), and
    /// `TZ=UTC` so the adapter prints reset times in UTC.
    #[must_use]
    pub fn claude_code_usage_probe() -> Self {
        let mut h = Self::claude_code("haiku");
        h.env.insert("TZ".into(), "UTC".into());
        h.mode_after_new = None;
        h.model = None;
        let meta = serde_json::json!({
            "claudeCode": { "options": {
                "strictMcpConfig": true,
                "tools": [],
                "persistSession": false,
            } }
        });
        h.session_meta = meta.as_object().cloned();
        h
    }

    /// OpenCode's built-in ACP server (`opencode acp`, verified with 2.0.12) with the
    /// given model as `<provider>/<model>` (e.g. `opencode/muse-spark-1.3-contributor-free`),
    /// see `docs/architecture/acp-harnesses.md` §7. The model is chosen with
    /// `set_config_option` (OpenCode's own default model is used until then), the
    /// mode is the built-in `build` agent, and the role prompt is prepended to the
    /// first prompt (OpenCode has no `_meta` system-prompt hook). The user's
    /// OpenCode configuration stays in effect, like `~/.claude` for Claude Code.
    #[must_use]
    pub fn opencode(model: &str) -> Self {
        Self {
            command: "opencode".into(),
            args: vec!["acp".into()],
            env: BTreeMap::from([("OPENCODE_DISABLE_AUTOUPDATE".into(), "1".into())]),
            env_remove: Vec::new(),
            mode_after_new: Some(OPENCODE_BUILD_MODE.into()),
            model: Some(ModelSelect {
                config_id: "model".into(),
                value: model.into(),
            }),
            effort: None,
            system_prompt: SystemPromptStyle::FirstPrompt,
            session_meta: None,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            client_info: None,
            permission_policy: PermissionPolicy::Default,
            system_prompt_note: None,
        }
    }

    /// Codex through `@agentclientprotocol/codex-acp` (2.0.0, verified against
    /// Codex 0.159), see `docs/architecture/acp-harnesses.md` §9. `codex_path`
    /// is the user's `codex` (`CODEX_PATH`; the adapter bundles an older one
    /// otherwise). Every role runs in `agent-full-access` mode (no approvals);
    /// the role prompt is prepended to the first prompt (no `_meta` hook). No
    /// model is set here: [`crate::agents::HarnessPreset::config`] passes the
    /// chosen model and effort through `CODEX_CONFIG`, because the adapter
    /// refuses `set_config_option` for models outside its own catalog (OpenRouter
    /// and other custom providers). The user's `CODEX_HOME` (config, skills,
    /// `AGENTS.md`) stays in effect.
    #[must_use]
    pub fn codex(codex_path: Option<&str>) -> Self {
        let mut env = BTreeMap::new();
        if let Some(path) = codex_path {
            env.insert(CODEX_PATH_ENV.to_string(), path.to_string());
        }
        Self {
            command: "npx".into(),
            args: vec!["-y".into(), CODEX_ACP.into()],
            env,
            env_remove: Vec::new(),
            mode_after_new: Some(CODEX_FULL_ACCESS_MODE.into()),
            model: None,
            effort: None,
            system_prompt: SystemPromptStyle::FirstPrompt,
            session_meta: None,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            client_info: None,
            permission_policy: PermissionPolicy::Default,
            system_prompt_note: None,
        }
    }

    /// Devin's ACP server (`devin acp`, stdio; <https://docs.devin.ai/desktop/acp>)
    /// at `command`. Every role runs in the `bypass`
    /// mode (no permission prompts), switched with `session/set_mode` because
    /// `--permission-mode` has no effect on `devin acp`. No model or effort is
    /// set here ([`crate::agents::HarnessPreset::config`] adds the chosen ones as
    /// config options; Devin's own default applies otherwise), and the role
    /// prompt is prepended to the first prompt (there is no `_meta` hook), with
    /// [`DEVIN_MCP_NOTE`] appended. The permission requests that still arrive are
    /// answered [`PermissionPolicy::OnceOnly`]. An inherited `RUST_LOG` is
    /// removed ([`DEVIN_ENV_REMOVE`]).
    #[must_use]
    pub fn devin(command: &str) -> Self {
        Self {
            command: command.into(),
            args: vec!["acp".into()],
            env: BTreeMap::new(),
            env_remove: DEVIN_ENV_REMOVE.iter().map(|v| (*v).to_string()).collect(),
            mode_after_new: Some(DEVIN_BYPASS_MODE.into()),
            model: None,
            effort: None,
            system_prompt: SystemPromptStyle::FirstPrompt,
            session_meta: None,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            client_info: None,
            permission_policy: PermissionPolicy::OnceOnly,
            system_prompt_note: Some(DEVIN_MCP_NOTE.into()),
        }
    }

    /// A bare harness running `command args...` with no mode/model configuration.
    #[must_use]
    pub fn plain(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            command: command.into(),
            args,
            env: BTreeMap::new(),
            env_remove: Vec::new(),
            mode_after_new: None,
            model: None,
            effort: None,
            system_prompt: SystemPromptStyle::MetaAppend,
            session_meta: None,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            client_info: None,
            permission_policy: PermissionPolicy::Default,
            system_prompt_note: None,
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
        let meta = serde_json::Value::Object(h.session_meta.clone().expect("meta"));
        assert_eq!(
            meta.pointer("/claudeCode/options/strictMcpConfig"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            h.env.get("ENABLE_TOOL_SEARCH").map(String::as_str),
            Some("false")
        );
    }

    #[test]
    fn opencode_defaults_use_build_mode_model_option_and_first_prompt() {
        let h = HarnessConfig::opencode("opencode/muse-spark-1.3-contributor-free");
        assert_eq!(
            (h.command.as_str(), h.args.as_slice()),
            ("opencode", &["acp".to_string()][..])
        );
        assert_eq!(h.mode_after_new.as_deref(), Some("build"));
        let model = h.model.expect("model select");
        assert_eq!(
            (model.config_id.as_str(), model.value.as_str()),
            ("model", "opencode/muse-spark-1.3-contributor-free")
        );
        assert_eq!(h.system_prompt, SystemPromptStyle::FirstPrompt);
        assert!(h.session_meta.is_none());
    }

    #[test]
    fn codex_defaults_use_full_access_first_prompt_and_the_users_codex() {
        let h = HarnessConfig::codex(Some("/home/u/.local/bin/codex"));
        assert_eq!(
            (h.command.as_str(), h.args.as_slice()),
            (
                "npx",
                &[
                    "-y".to_string(),
                    "@agentclientprotocol/codex-acp@2.0.0".to_string()
                ][..]
            ),
            "the adapter version is pinned exactly"
        );
        assert_eq!(h.mode_after_new.as_deref(), Some("agent-full-access"));
        assert_eq!(h.system_prompt, SystemPromptStyle::FirstPrompt);
        assert_eq!(
            h.env.get("CODEX_PATH").map(String::as_str),
            Some("/home/u/.local/bin/codex")
        );
        assert!(h.model.is_none() && h.effort.is_none() && h.session_meta.is_none());
        assert!(h.env_remove.is_empty(), "CODEX_HOME is inherited as is");
        assert!(!HarnessConfig::codex(None).env.contains_key("CODEX_PATH"));
    }

    #[test]
    fn devin_defaults_use_acp_bypass_first_prompt_and_once_only_permissions() {
        let h = HarnessConfig::devin("/home/u/.local/bin/devin");
        assert_eq!(
            (h.command.as_str(), h.args.as_slice()),
            ("/home/u/.local/bin/devin", &["acp".to_string()][..])
        );
        assert_eq!(h.mode_after_new.as_deref(), Some("bypass"));
        assert_eq!(h.system_prompt, SystemPromptStyle::FirstPrompt);
        assert!(h.model.is_none() && h.effort.is_none() && h.session_meta.is_none());
        assert!(h.env.is_empty());
        assert_eq!(h.env_remove, ["RUST_LOG"], "Devin's own log stays readable");
        assert_eq!(h.permission_policy, PermissionPolicy::OnceOnly);
        let note = h.system_prompt_note.expect("the MCP note");
        assert!(note.contains("`yhtye`") && note.contains("mcp_list_tools"));
        assert_eq!(h.client_info, None, "no other product's name is claimed");
    }

    #[test]
    fn other_harnesses_keep_the_default_client_info_permission_policy_and_prompt() {
        for h in [
            HarnessConfig::claude_code("haiku"),
            HarnessConfig::opencode("opencode/x"),
            HarnessConfig::codex(None),
            HarnessConfig::plain("x", vec![]),
        ] {
            assert_eq!(h.client_info, None);
            assert_eq!(h.permission_policy, PermissionPolicy::Default);
            assert_eq!(
                h.system_prompt_note, None,
                "only Devin adds to the role prompt"
            );
            assert!(!h.env_remove.iter().any(|v| v == "RUST_LOG"));
        }
    }

    #[test]
    fn client_info_and_permission_policy_are_optional_in_serialized_configs() {
        let h: HarnessConfig = serde_json::from_str(
            r#"{"command":"x","client_info":{"name":"n","version":"1"},"permission_policy":"once_only"}"#,
        )
        .expect("parse");
        assert_eq!(
            h.client_info,
            Some(ClientInfoOverride {
                name: "n".into(),
                version: "1".into()
            })
        );
        assert_eq!(h.permission_policy, PermissionPolicy::OnceOnly);
        let json = serde_json::to_value(HarnessConfig::plain("x", vec![])).expect("serialize");
        assert!(
            ["client_info", "permission_policy", "system_prompt_note"]
                .iter()
                .all(|k| json.get(k).is_none())
        );
        let back: HarnessConfig = serde_json::from_value(
            serde_json::to_value(HarnessConfig::devin("devin")).expect("serialize"),
        )
        .expect("round trip");
        assert_eq!(back, HarnessConfig::devin("devin"));
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
