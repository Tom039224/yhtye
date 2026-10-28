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
    /// `session/set_mode` sent right after `session/new` / `session/load` (not
    /// when the agent already reports that mode as current).
    #[serde(default)]
    pub mode_after_new: Option<String>,
    /// `session/set_config_option` sent after the mode is set (not when the agent
    /// already reports that value as current).
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
    /// Appended to the startup error when `session/new` / `session/load` answers
    /// that authentication is required, for a harness Yhtye cannot log in for
    /// (it never calls `authenticate`) and whose own message does not say what
    /// to do from here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_hint: Option<String>,
    /// Secret environment variables that go to this harness only, each under
    /// the name the harness reads (see [`SecretBinding`]). The secret is not
    /// part of the variables every agent gets, and the harness does not start
    /// without it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_bindings: Vec<SecretBinding>,
}

/// A secret environment variable handed to one harness under another name:
/// the value registered as `secret` (Settings › Secret environment variables)
/// reaches only the harnesses that bind it, as `env`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretBinding {
    /// The registered secret's name.
    pub secret: String,
    /// The environment variable the agent process gets it as.
    pub env: String,
    /// What the user sees when the secret is not registered: who needs it
    /// (`Claude Code (API)`).
    pub needed_by: String,
}

impl SecretBinding {
    /// What to tell the user when `secret` is not registered or has no value.
    #[must_use]
    pub fn missing_message(&self) -> String {
        format!(
            "{} には秘密の環境変数 {} の登録が必要です (設定 › 秘密の環境変数)",
            self.needed_by, self.secret
        )
    }
}

/// The Claude Code ACP adapter run through `npx` (an exact version).
pub const CLAUDE_AGENT_ACP: &str = "@agentclientprotocol/claude-agent-acp@0.84.0";

/// The secret that holds the Anthropic API key of `Claude Code (API)`. Only
/// that harness receives it, as `ANTHROPIC_API_KEY`
/// ([`HarnessConfig::claude_code_api`]); no other agent process gets it.
pub const CLAUDE_API_KEY_SECRET: &str = "YHTYE_ANTHROPIC_API_KEY";

/// The variable Claude Code reads an Anthropic API key from.
pub const ANTHROPIC_API_KEY_ENV: &str = "ANTHROPIC_API_KEY";

/// Secrets that only the harness binding them gets, never the agents in
/// general ([`SecretBinding`]).
pub const HARNESS_ONLY_SECRETS: [&str; 1] = [CLAUDE_API_KEY_SECRET];

/// Variables removed from the subscription Claude Code: either of them
/// would bill the API (or another account) instead of the user's claude.ai
/// subscription, even when it only comes from the shell Yhtye was started in.
pub const CLAUDE_CODE_SUBSCRIPTION_ENV_REMOVE: [&str; 2] =
    [ANTHROPIC_API_KEY_ENV, "ANTHROPIC_AUTH_TOKEN"];

/// Variables removed from Claude Code (API): they are used before an API key,
/// so the key would not be what bills.
pub const CLAUDE_CODE_API_ENV_REMOVE: [&str; 2] =
    ["ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"];

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

/// The model Yhtye runs MiniMax Code's sessions on when a choice names none
/// (`mcode`'s own default is whatever the user's `~/.minimax/config.yaml` says,
/// possibly a model that costs more): the cheapest, the Flash Preview with
/// thinking. The value has the form `m:minimax:<Model>:v:<variant>`.
pub const MINIMAX_CODE_DEFAULT_MODEL: &str = "m:minimax:MiniMax-M3.1-Flash-Preview:v:thinking";

/// Models `mcode acp` lists but refuses when they are selected (`-32603
/// "Invalid model reasoning"`: the Flash Preview without thinking), so the
/// model listing leaves them out.
pub const MINIMAX_CODE_UNUSABLE_MODELS: [&str; 1] = ["m:minimax:MiniMax-M3.1-Flash-Preview:v:"];

/// The id of MiniMax Code's effort option (category `thought_level`). It exists
/// only while a model with effort levels is selected, so the effort is set
/// after the model.
pub const MINIMAX_CODE_EFFORT_CONFIG_ID: &str = "thinkingEffort";
/// The arguments of Grok Build's ACP server: `grok agent --no-leader stdio`.
/// `--no-leader` keeps every agent in a process of its own even when the
/// user's `~/.grok/config.toml` enables the shared leader (`[cli] use_leader`),
/// so Yhtye's process group, working directory and environment are the agent's.
pub const GROK_BUILD_ARGS: [&str; 3] = ["agent", "--no-leader", "stdio"];

/// The id of Grok Build's effort option (category `thought_level`; values
/// `xhigh` / `high` / `medium` / `low` with grok 1.0.46).
pub const GROK_BUILD_EFFORT_CONFIG_ID: &str = "reasoning_effort";

/// Inherited variables removed from Grok Build's environment: `grok` logs with
/// `tracing` and honours `RUST_LOG`, so a value meant for Yhtye would flood the
/// agent's stderr (`trace`: megabytes per second) or filter its log.
pub const GROK_BUILD_ENV_REMOVE: [&str; 1] = ["RUST_LOG"];

/// The model Yhtye runs Google Antigravity's sessions on when a choice names
/// none (the server's own default is a constant of its binary,
/// `gemini-3.8-flash-high`): the cheapest, Flash with the lowest thinking level.
/// The thinking level is part of the model id (`gemini-3.8-flash-high|medium|low`),
/// the server has no separate effort option.
pub const ANTIGRAVITY_DEFAULT_MODEL: &str = "gemini-3.8-flash-low";

/// Appended to the startup error of Google Antigravity when it answers that
/// authentication is required ([`HarnessConfig::login_hint`]). `agy_acp_server`
/// has no login command (started in a terminal it only waits for an ACP client):
/// the sign-in is started by `authenticate`, or by `session/new` once
/// `auth.type` is `oauth-personal` in its `settings.json`
/// (`docs/architecture/acp-harnesses.md` §13.7).
pub const ANTIGRAVITY_LOGIN_HINT: &str = "\
Google Antigravity にログインしていません (Yhtye は代わりにログインしません)。\
`$GEMINI_HOME/antigravity-acp/settings.json` (GEMINI_HOME が無ければ \
`~/.gemini/antigravity-acp/settings.json`) に `{\"auth\": {\"type\": \"oauth-personal\"}}` を書いて \
もう一度始めると、ブラウザで Google のログインが開きます \
(Zed など別の ACP クライアントで一度ログインしてもよい。API キーを使うなら type を \
`gemini-api-key` にして環境変数 GEMINI_API_KEY を渡す)。詳しくは SETUP.md の「Google Antigravity」。";

/// The pi ACP adapter run through `npx` (an exact version, like
/// [`CLAUDE_AGENT_ACP`]; `docs/architecture/acp-harnesses.md` §14). pi has no ACP
/// of its own: the adapter embeds the pi SDK and uses the user's `~/.pi/agent`
/// (settings, credentials, extensions, sessions) as `pi` does.
pub const PI_ACP: &str = "@openma/pi-acp@0.1.8";

/// The adapter's environment variable with the model of new sessions,
/// `<provider>/<id>` with an optional `:<thinking level>` (`orcarouter/x/y:low`).
/// pi's model and thinking level are set only through it: the adapter saves
/// every `session/set_config_option` (model, thinking) to the user's own
/// `~/.pi/agent/settings.json` as pi's defaults.
pub const PI_ACP_MODEL_ENV: &str = "PI_ACP_MODEL";

/// The adapter's environment variable that skips its startup banner (a
/// `agent_message_chunk` with the pi version and loaded resources).
pub const PI_ACP_QUIET_STARTUP_ENV: &str = "PI_ACP_QUIET_STARTUP";

/// The id of pi's effort option (category `thought_level`). It exists only
/// while the model reasons, and its values depend on the model (`off`, `low`,
/// ... `xhigh`).
pub const PI_EFFORT_CONFIG_ID: &str = "thinking";

/// Appended to every role prompt of pi ([`HarnessConfig::system_prompt_note`]):
/// pi declares MCP tools only as `tool_search` first, so the tool names of the
/// role prompts (`mcp__yhtye__report_step_done`) are not tools until they are
/// loaded (`docs/architecture/acp-harnesses.md` §14).
pub const PI_MCP_NOTE: &str = "\
Note for pi: Yhtye's tools (`report_step_done`, `help`, `create_group`, ...) \
are on the MCP server named `yhtye` and are deferred: they are not among your \
tools until you load them. First load them with `tool_search` (for example \
`report_step_done`); once loaded, call a tool as `mcp__yhtye__<name>` \
(`mcp__yhtye__report_step_done`).";

/// Appended to the startup error of pi when it answers that authentication is
/// required ([`HarnessConfig::login_hint`]): the adapter uses the user's own
/// pi credentials and Yhtye never calls its `authenticate`.
pub const PI_LOGIN_HINT: &str = "\
pi にログインしていません (Yhtye は代わりにログインしません)。ターミナルで `pi` を起動して \
`/login` するか、使うプロバイダの API キー (例: `ANTHROPIC_API_KEY`) を環境変数で設定して \
もう一度始めてください。";

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
    /// `*_always` options that would change how the session runs; Grok Build
    /// offers `allow_always_*` and `allow_edits_for_session`).
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
    /// This is the subscription Claude Code (the user's claude.ai login): an API key
    /// or token in the inherited environment is removed
    /// ([`CLAUDE_CODE_SUBSCRIPTION_ENV_REMOVE`]) so it never bills the API.
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
            env_remove: CLAUDE_CODE_SUBSCRIPTION_ENV_REMOVE
                .map(String::from)
                .to_vec(),
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
            login_hint: None,
            secret_bindings: Vec::new(),
        }
    }

    /// Claude Code billed to an Anthropic API key (`Claude Code (API)`): the
    /// subscription config, except that the secret [`CLAUDE_API_KEY_SECRET`]
    /// is the agent's `ANTHROPIC_API_KEY` (nothing starts without it), and the
    /// variables that would be used before the key are removed
    /// ([`CLAUDE_CODE_API_ENV_REMOVE`]).
    #[must_use]
    pub fn claude_code_api(model: &str) -> Self {
        let mut h = Self::claude_code(model);
        h.env_remove = CLAUDE_CODE_API_ENV_REMOVE.map(String::from).to_vec();
        h.secret_bindings = vec![SecretBinding {
            secret: CLAUDE_API_KEY_SECRET.into(),
            env: ANTHROPIC_API_KEY_ENV.into(),
            needed_by: "Claude Code (API)".into(),
        }];
        h
    }

    /// [`HarnessConfig::claude_code`] for reading subscription usage with the
    /// local `/usage` command (`crate::usage`): no tools, no MCP servers, the
    /// session is not saved to `~/.claude/projects` (`persistSession: false`),
    /// no mode / model switching (the command never reaches the model), and
    /// `TZ=UTC` so the adapter prints reset times in UTC.
    #[must_use]
    pub fn claude_code_usage_probe() -> Self {
        Self::claude_code("haiku").into_listing_session()
    }

    /// [`HarnessConfig::claude_code_api`] as a listing session (the models
    /// and efforts), which [`HarnessConfig::claude_code_usage_probe`] is for
    /// the subscription: no tools, nothing saved, no mode / model switching.
    #[must_use]
    pub fn claude_code_api_probe() -> Self {
        Self::claude_code_api("haiku").into_listing_session()
    }

    fn into_listing_session(mut self) -> Self {
        self.env.insert("TZ".into(), "UTC".into());
        self.mode_after_new = None;
        self.model = None;
        let meta = serde_json::json!({
            "claudeCode": { "options": {
                "strictMcpConfig": true,
                "tools": [],
                "persistSession": false,
            } }
        });
        self.session_meta = meta.as_object().cloned();
        self
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
            login_hint: None,
            secret_bindings: Vec::new(),
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
            login_hint: None,
            secret_bindings: Vec::new(),
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
            login_hint: None,
            secret_bindings: Vec::new(),
        }
    }

    /// MiniMax Code's ACP server (`mcode acp`, stdio) at `command`, see
    /// `docs/architecture/acp-harnesses.md` §11. Neither the mode nor the
    /// `permissionMode` option is touched: `permissionMode` is written to the
    /// user's global `~/.minimax/config.yaml` when it is set, and the default
    /// (`auto`: writes inside the working directory and MCP calls need no
    /// request) already fits. The model is set to
    /// [`MINIMAX_CODE_DEFAULT_MODEL`] unless a choice names another
    /// ([`crate::agents::HarnessPreset::config`]), the role prompt is prepended
    /// to the first prompt (`_meta.systemPrompt` is ignored), and the permission
    /// requests that still arrive are answered [`PermissionPolicy::OnceOnly`]
    /// (`allow-always` would be saved to the user's settings).
    #[must_use]
    pub fn minimax_code(command: &str) -> Self {
        Self {
            command: command.into(),
            args: vec!["acp".into()],
            env: BTreeMap::new(),
            env_remove: Vec::new(),
            mode_after_new: None,
            model: Some(ModelSelect {
                config_id: "model".into(),
                value: MINIMAX_CODE_DEFAULT_MODEL.into(),
            }),
            effort: None,
            system_prompt: SystemPromptStyle::FirstPrompt,
            session_meta: None,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            client_info: None,
            permission_policy: PermissionPolicy::OnceOnly,
            system_prompt_note: None,
            login_hint: None,
            secret_bindings: Vec::new(),
        }
    }

    /// Google Antigravity's ACP server (`agy_acp_server`, stdio, no arguments) at
    /// `command`, see `docs/architecture/acp-harnesses.md` §13. The mode is left
    /// at `default`, the environment (`GEMINI_HOME` included) is inherited as is,
    /// and no `clientInfo` is claimed (`yhtye` is accepted; only the names of a
    /// few editors unlock other vendors' models). The model is set to
    /// [`ANTIGRAVITY_DEFAULT_MODEL`] unless a choice names another
    /// ([`crate::agents::HarnessPreset::config`]); there is no effort option (the
    /// thinking level is part of the model id). The role prompt is prepended to
    /// the first prompt (the server has no hook for it), the permission requests
    /// that arrive are answered [`PermissionPolicy::OnceOnly`] (`allow_always` is
    /// saved with the session), and an authentication error is followed by
    /// [`ANTIGRAVITY_LOGIN_HINT`].
    #[must_use]
    pub fn antigravity(command: &str) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            env_remove: Vec::new(),
            mode_after_new: None,
            model: Some(ModelSelect {
                config_id: "model".into(),
                value: ANTIGRAVITY_DEFAULT_MODEL.into(),
            }),
            effort: None,
            system_prompt: SystemPromptStyle::FirstPrompt,
            session_meta: None,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            client_info: None,
            permission_policy: PermissionPolicy::OnceOnly,
            system_prompt_note: None,
            login_hint: Some(ANTIGRAVITY_LOGIN_HINT.into()),
            secret_bindings: Vec::new(),
        }
    }

    /// Grok Build's ACP server (`grok agent --no-leader stdio`, [`GROK_BUILD_ARGS`])
    /// at `command`, see `docs/architecture/acp-harnesses.md` §12. `grok`
    /// advertises no modes, so none is set; it is not started with
    /// `--always-approve` either: the permission requests that reach Yhtye are
    /// answered [`PermissionPolicy::OnceOnly`] (its `allow_always*` options
    /// persist grants). No model or effort is set here
    /// ([`crate::agents::HarnessPreset::config`] adds the chosen ones as config
    /// options; `grok`'s own default applies otherwise), the role prompt is
    /// prepended to the first prompt, and an inherited `RUST_LOG` is removed
    /// ([`GROK_BUILD_ENV_REMOVE`]).
    #[must_use]
    pub fn grok_build(command: &str) -> Self {
        Self {
            command: command.into(),
            args: GROK_BUILD_ARGS.iter().map(|a| (*a).to_string()).collect(),
            env: BTreeMap::new(),
            env_remove: GROK_BUILD_ENV_REMOVE
                .iter()
                .map(|v| (*v).to_string())
                .collect(),
            mode_after_new: None,
            model: None,
            effort: None,
            system_prompt: SystemPromptStyle::FirstPrompt,
            session_meta: None,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            client_info: None,
            permission_policy: PermissionPolicy::OnceOnly,
            system_prompt_note: None,
            login_hint: None,
            secret_bindings: Vec::new(),
        }
    }

    /// pi through `@openma/pi-acp` (`npx`, [`PI_ACP`]), see
    /// `docs/architecture/acp-harnesses.md` §14. pi has no modes, so none is set,
    /// and the model and thinking level are not set as config options either:
    /// the adapter would save them to the user's own `settings.json`
    /// ([`crate::agents::HarnessPreset::config`] passes the chosen ones in
    /// [`PI_ACP_MODEL_ENV`]; nothing is chosen here, so pi's own default applies
    /// otherwise). The startup banner is switched off ([`PI_ACP_QUIET_STARTUP_ENV`]);
    /// the rest of the user's environment (`PI_CODING_AGENT_DIR`, ...) is
    /// inherited as it is. The role prompt is prepended to the first prompt
    /// (`_meta.systemPrompt` is ignored), with [`PI_MCP_NOTE`] appended. pi asks
    /// no tool approvals itself; the extension dialogs that arrive as permission
    /// requests are answered [`PermissionPolicy::OnceOnly`], and an authentication
    /// error is followed by [`PI_LOGIN_HINT`].
    #[must_use]
    pub fn pi() -> Self {
        Self {
            command: "npx".into(),
            args: vec!["-y".into(), PI_ACP.into()],
            env: BTreeMap::from([(PI_ACP_QUIET_STARTUP_ENV.into(), "1".into())]),
            env_remove: Vec::new(),
            mode_after_new: None,
            model: None,
            effort: None,
            system_prompt: SystemPromptStyle::FirstPrompt,
            session_meta: None,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            client_info: None,
            permission_policy: PermissionPolicy::OnceOnly,
            system_prompt_note: Some(PI_MCP_NOTE.into()),
            login_hint: Some(PI_LOGIN_HINT.into()),
            secret_bindings: Vec::new(),
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
            login_hint: None,
            secret_bindings: Vec::new(),
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
    fn the_subscription_claude_code_never_inherits_an_api_credential() {
        for h in [
            HarnessConfig::claude_code("haiku"),
            HarnessConfig::claude_code_usage_probe(),
        ] {
            assert_eq!(h.env_remove, ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]);
            assert!(
                h.secret_bindings.is_empty(),
                "it is never given the API key"
            );
            assert!(!h.env.contains_key("ANTHROPIC_API_KEY"));
        }
    }

    #[test]
    fn claude_code_api_gets_the_key_from_its_secret_and_loses_the_other_credentials() {
        for h in [
            HarnessConfig::claude_code_api("haiku"),
            HarnessConfig::claude_code_api_probe(),
        ] {
            assert_eq!(
                h.secret_bindings,
                [SecretBinding {
                    secret: "YHTYE_ANTHROPIC_API_KEY".into(),
                    env: "ANTHROPIC_API_KEY".into(),
                    needed_by: "Claude Code (API)".into(),
                }]
            );
            assert_eq!(
                h.env_remove,
                ["ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"]
            );
            assert!(
                !h.env.contains_key("ANTHROPIC_API_KEY"),
                "the key is never part of the config"
            );
            let plain = HarnessConfig::claude_code("haiku");
            assert_eq!(h.args, plain.args);
            assert_eq!(
                h.env.get("ENABLE_TOOL_SEARCH"),
                plain.env.get("ENABLE_TOOL_SEARCH")
            );
        }
        let api = HarnessConfig::claude_code_api("haiku");
        assert_eq!(api.mode_after_new.as_deref(), Some("bypassPermissions"));
        assert_eq!(api.model.map(|m| m.value), Some("haiku".to_string()));
    }

    #[test]
    fn the_claude_code_listing_sessions_switch_nothing_and_save_nothing() {
        for h in [
            HarnessConfig::claude_code_usage_probe(),
            HarnessConfig::claude_code_api_probe(),
        ] {
            assert_eq!((h.mode_after_new, h.model), (None, None));
            assert_eq!(h.env.get("TZ").map(String::as_str), Some("UTC"));
            let meta = serde_json::Value::Object(h.session_meta.expect("meta"));
            assert_eq!(
                meta.pointer("/claudeCode/options/persistSession"),
                Some(&serde_json::json!(false))
            );
        }
    }

    #[test]
    fn a_missing_secret_is_reported_with_who_needs_it_and_never_a_value() {
        let message = HarnessConfig::claude_code_api("haiku").secret_bindings[0].missing_message();
        assert_eq!(
            message,
            "Claude Code (API) には秘密の環境変数 YHTYE_ANTHROPIC_API_KEY の登録が必要です (設定 › 秘密の環境変数)"
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
    fn antigravity_runs_without_arguments_touches_nothing_and_points_to_the_login() {
        let h = HarnessConfig::antigravity("/opt/agy/agy_acp_server.par");
        assert_eq!(
            (h.command.as_str(), h.args.as_slice()),
            ("/opt/agy/agy_acp_server.par", &[][..]),
            "no arguments (the registry's --uid= is not needed)"
        );
        assert!(h.env.is_empty(), "GEMINI_HOME is the user's own");
        assert!(h.env_remove.is_empty());
        assert_eq!(h.mode_after_new, None, "the mode stays `default`");
        let model = h.model.expect("the cheapest model");
        assert_eq!(
            (model.config_id.as_str(), model.value.as_str()),
            ("model", "gemini-3.8-flash-low")
        );
        assert!(h.effort.is_none() && h.session_meta.is_none());
        assert_eq!(h.system_prompt, SystemPromptStyle::FirstPrompt);
        assert_eq!(h.permission_policy, PermissionPolicy::OnceOnly);
        assert_eq!(h.system_prompt_note, None);
        assert_eq!(h.client_info, None, "no other product's name is claimed");
        let hint = h.login_hint.expect("how to log in");
        assert!(hint.contains("antigravity-acp/settings.json") && hint.contains("oauth-personal"));
    }

    #[test]
    fn grok_build_runs_its_own_stdio_agent_without_modes_or_always_approve() {
        let h = HarnessConfig::grok_build("/home/u/.grok/bin/grok");
        assert_eq!(h.command, "/home/u/.grok/bin/grok");
        assert_eq!(
            h.args,
            ["agent", "--no-leader", "stdio"],
            "never the shared leader, never --always-approve"
        );
        assert_eq!(h.mode_after_new, None, "grok advertises no modes");
        assert!(h.model.is_none() && h.effort.is_none() && h.session_meta.is_none());
        assert_eq!(h.system_prompt, SystemPromptStyle::FirstPrompt);
        assert_eq!(h.permission_policy, PermissionPolicy::OnceOnly);
        assert_eq!(h.system_prompt_note, None, "its MCP tools keep their names");
        assert!(h.env.is_empty());
        assert_eq!(h.env_remove, ["RUST_LOG"]);
        assert_eq!(h.client_info, None);
        let back: HarnessConfig =
            serde_json::from_value(serde_json::to_value(&h).expect("serialize"))
                .expect("round trip");
        assert_eq!(back, h);
    }

    #[test]
    fn pi_runs_the_pinned_adapter_and_sets_neither_mode_model_nor_effort() {
        let h = HarnessConfig::pi();
        assert_eq!(
            (h.command.as_str(), h.args.as_slice()),
            (
                "npx",
                &["-y".to_string(), "@openma/pi-acp@0.1.8".to_string()][..]
            ),
            "the adapter version is pinned exactly"
        );
        assert_eq!(
            h.env,
            BTreeMap::from([("PI_ACP_QUIET_STARTUP".to_string(), "1".to_string())]),
            "only the banner is switched off: the user's pi environment is inherited"
        );
        assert!(h.env_remove.is_empty());
        assert_eq!(h.mode_after_new, None, "pi has no modes");
        assert!(
            h.model.is_none() && h.effort.is_none(),
            "set_config_option would save them to ~/.pi/agent/settings.json"
        );
        assert_eq!(h.system_prompt, SystemPromptStyle::FirstPrompt);
        assert_eq!(h.permission_policy, PermissionPolicy::OnceOnly);
        assert!(h.session_meta.is_none());
        assert_eq!(h.client_info, None, "no other product's name is claimed");
        let note = h.system_prompt_note.expect("the MCP note");
        assert!(
            note.contains("tool_search") && note.contains("mcp__yhtye__<name>"),
            "{note}"
        );
        let hint = h.login_hint.expect("how to log in");
        assert!(
            hint.contains("`/login`") && hint.contains("API キー"),
            "{hint}"
        );
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
            assert_eq!(
                h.login_hint, None,
                "only Antigravity and pi point to a login"
            );
            assert_eq!(h.permission_policy, PermissionPolicy::Default);
            assert_eq!(
                h.system_prompt_note, None,
                "only Devin and pi add to the role prompt"
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
