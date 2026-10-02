//! Scenario format (`YHTYE_FAKE_SCRIPT=<json>`).
//!
//! ```json
//! {
//!   "load_session": true,
//!   "modes": ["default", "bypassPermissions"],
//!   "models": ["default", "haiku"],
//!   "efforts": {"haiku": ["low", "high"]},   // models with an `effort` option
//!   "effort_id": "reasoning",   // id of that option (default "effort"; category stays thought_level)
//!   "permission_modes": ["auto", "default"],  // adds a `permissionMode` option (the first is current)
//!   "fail_at": null,            // "initialize" | "session/new" | ... → JSON-RPC error
//!   "fail_kind": "internal",    // the error: "internal" (default) | "auth_required"
//!   "fail_message": null,       // its data (default "fake: scripted failure at <step>")
//!   "exit_at": null,            // same steps → print to stderr and exit(2)
//!   "hang_at": null,            // same steps → never answer
//!   "permission_options": [{"id": "allow", "kind": "allow_once"}],  // for "ask_permission"
//!   "vendor_requests": false,   // true: every turn starts with `_cognition.ai/request_diagnostics`
//!   "turns": [
//!     { "match": "hello", "actions": [ {"message": "hi"}, {"end": "end_turn"} ] },
//!     { "actions": [ {"sleep": 50}, "wait_cancel" ] }
//!   ]
//! }
//! ```
//!
//! Devin-like behaviour: `effort_id` renames the effort option, `"ask_permission"`
//! offers `permission_options` (`switch_*` / `*_always` ids included, as Devin does),
//! `vendor_requests` makes the agent send Devin's diagnostics request and report
//! the client's answer as `vendor:request_diagnostics:ok:<json>` (or `:error:<msg>`),
//! `"report_client_info"` reports the `clientInfo` of `initialize`
//! (`client_info:name=<n>;version=<v>`), and `fail_kind` + `fail_message` make
//! `fail_at` an authentication error (`"login required"`).
//!
//! MiniMax-Code-like behaviour: `permission_modes` adds the option `permissionMode`
//! (category `_permission`) that a client must not write (it would reach the user's
//! global settings), and `"report_writes"` reports every `session/set_mode` and
//! `session/set_config_option` received, in order, as `writes:modes=<ids>;options=<ids>`.
//!
//! MCP actions: `{"mcp_call": {"tool": "create_group", "args": {"title": "x"}}}`
//! and `"mcp_list"` use the first HTTP MCP server passed in `session/new`.
//!
//! A prompt runs the first turn whose `match` is a substring of the prompt text;
//! otherwise the next turn without `match`, in order. A turn without a trailing
//! `end` ends with `end_turn`.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    #[serde(default = "yes")]
    pub load_session: bool,
    #[serde(default = "default_modes")]
    pub modes: Vec<String>,
    #[serde(default = "default_models")]
    pub models: Vec<String>,
    /// Models that have an `effort` select option, with its values (after a
    /// leading `default`, like Claude Code's). Changing the model rebuilds it.
    #[serde(default)]
    pub efforts: std::collections::BTreeMap<String, Vec<String>>,
    /// Id of the `effort` option (its category stays `thought_level`, so a
    /// client that looks the option up by category finds it under any id).
    #[serde(default)]
    pub effort_id: Option<String>,
    /// The values of a `permissionMode` select option (category `_permission`),
    /// the first being current; none: the agent has no such option.
    #[serde(default)]
    pub permission_modes: Vec<String>,
    #[serde(default)]
    pub fail_at: Option<String>,
    /// What `fail_at` answers with.
    #[serde(default)]
    pub fail_kind: FailKind,
    /// The data of the `fail_at` error (a default text names the step).
    #[serde(default)]
    pub fail_message: Option<String>,
    #[serde(default)]
    pub exit_at: Option<String>,
    #[serde(default)]
    pub hang_at: Option<String>,
    /// The options the `ask_permission` action offers.
    #[serde(default)]
    pub permission_options: Vec<PermissionChoice>,
    /// Every turn starts with Devin's `_cognition.ai/request_diagnostics`
    /// request to the client; the outcome is reported as a message.
    #[serde(default)]
    pub vendor_requests: bool,
    #[serde(default)]
    pub turns: Vec<Turn>,
}

/// The JSON-RPC error `fail_at` answers with.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailKind {
    #[default]
    Internal,
    /// ACP's "authentication required" (`-32000`), like an agent that is not logged in.
    AuthRequired,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    #[serde(default, rename = "match")]
    pub matches: Option<String>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// `agent_message_chunk` with this text.
    Message(String),
    /// `agent_thought_chunk` with this text.
    Thought(String),
    ToolCall {
        id: String,
        title: String,
    },
    ToolCallUpdate {
        id: String,
        status: String,
    },
    /// `plan` with these entries (status `pending`).
    Plan(Vec<String>),
    /// Any `update` object, sent verbatim (e.g. kinds without a dedicated action).
    Update(serde_json::Value),
    /// `session/request_permission`; the outcome is reported back as a message
    /// `permission:selected:<id>` or `permission:cancelled`.
    RequestPermission(Vec<PermissionChoice>),
    /// [`Action::RequestPermission`] with the scenario's `permission_options`.
    AskPermission,
    Sleep(u64),
    /// Waits until `session/cancel` arrives.
    WaitCancel,
    /// Ends the turn with this stop reason (`end_turn`, `max_tokens`, `refusal`, ...).
    End(String),
    /// `exit(code)` immediately.
    Crash(i32),
    /// Spawns `sleep 600` in the agent's process group; reports `child_pid:<pid>`.
    SpawnChild,
    /// Reports `state:mode=<m>;model=<v>;system_prompt=<s>;prompt=<text>`.
    ReportState,
    /// Reports `writes:modes=<ids>;options=<ids>`: the mode ids of the
    /// `session/set_mode` requests and the config ids of the
    /// `session/set_config_option` requests received so far (comma-separated,
    /// in order).
    ReportWrites,
    /// Reports `client_info:name=<n>;version=<v>` (the `clientInfo` of `initialize`,
    /// `<none>` when the client sent none).
    ReportClientInfo,
    /// Reports `meta:<json>` (the `_meta` received in `session/new` / `session/load`).
    ReportMeta,
    /// Writes `text` to `path` (relative to the working directory).
    WriteFile {
        path: String,
        text: String,
    },
    /// Runs a command (`["git", "merge", "x"]`) in the working directory;
    /// reports `run:<exit code>` as a message.
    Run(Vec<String>),
    /// Calls a tool on the session's HTTP MCP server; `${name}` placeholders in
    /// `args` are substituted (see `mcp.rs`). Reports `mcp:<tool>:ok:<json>`,
    /// `mcp:<tool>:error:<json>` or `mcp:<tool>:failed:<reason>` as a message.
    McpCall {
        tool: String,
        #[serde(default)]
        args: serde_json::Value,
    },
    /// Reports `mcp:tools:<name>,<name>,...` (the session's `tools/list`).
    McpList,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PermissionChoice {
    pub id: String,
    pub kind: String,
}

fn yes() -> bool {
    true
}

fn default_modes() -> Vec<String> {
    vec!["default".into(), "bypassPermissions".into()]
}

fn default_models() -> Vec<String> {
    vec!["default".into(), "haiku".into()]
}

impl Scenario {
    /// The actions for a prompt; `next_seq` tracks the sequential (unmatched) turns.
    pub fn pick(&self, prompt: &str, next_seq: &mut usize) -> Vec<Action> {
        let matched = self
            .turns
            .iter()
            .find(|t| t.matches.as_deref().is_some_and(|m| prompt.contains(m)));
        if let Some(turn) = matched {
            return turn.actions.clone();
        }
        let sequential = self
            .turns
            .iter()
            .filter(|t| t.matches.is_none())
            .nth(*next_seq);
        *next_seq += 1;
        sequential.map_or_else(
            || {
                vec![Action::Message(format!(
                    "fake: no scenario turn for {prompt:?}"
                ))]
            },
            |t| t.actions.clone(),
        )
    }

    /// Whether startup step `step` should fail / exit / hang.
    pub fn at(&self, which: &Option<String>, step: &str) -> bool {
        which.as_deref() == Some(step)
    }
}
