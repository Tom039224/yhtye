//! ACP client: launches a harness process, opens one session on it and turns
//! everything the agent reports into typed [`AgentEvent`]s.
//!
//! See `docs/architecture/core-design.md` §3 and `docs/architecture/acp-harnesses.md`.

mod config;
mod events;
mod handle;
mod permission;
mod process;
mod session;
mod startup;

pub use config::{
    CODEX_ACP, CODEX_CONFIG_ENV, CODEX_FULL_ACCESS_MODE, CODEX_PATH_ENV, ClientInfoOverride,
    DEFAULT_STARTUP_TIMEOUT, DEVIN_BYPASS_MODE, DEVIN_ENV_REMOVE, DEVIN_MCP_NOTE, EFFORT_CONFIG_ID,
    GROK_BUILD_ARGS, GROK_BUILD_EFFORT_CONFIG_ID, GROK_BUILD_ENV_REMOVE, HarnessConfig,
    MINIMAX_CODE_DEFAULT_MODEL, MINIMAX_CODE_EFFORT_CONFIG_ID, MINIMAX_CODE_UNUSABLE_MODELS,
    ModelSelect, OPENCODE_BUILD_MODE, PermissionPolicy, SystemPromptStyle,
};
pub use events::{AgentError, AgentEvent, AgentInfo, AgentOutput, config_value, effort_option};
pub use handle::{AgentHandle, SpawnOptions, spawn_agent};
pub use permission::{choose_permission, outcome_for};
pub(crate) use process::PRIVATE_ENV;

/// ACP schema types used in this module's public API.
pub use agent_client_protocol::schema::v1 as schema;
