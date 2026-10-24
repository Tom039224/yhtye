//! ACP client: launches a harness process, opens one session on it and turns
//! everything the agent reports into typed [`AgentEvent`]s.
//!
//! See `docs/architecture/core-design.md` §3 and `docs/architecture/acp-harnesses.md`.

mod config;
mod events;
mod handle;
#[cfg(all(test, target_os = "linux"))]
mod live_tests;
mod orphan;
mod parent_death;
mod permission;
mod process;
mod session;
mod startup;
mod steering;

pub use config::{
    ANTIGRAVITY_DEFAULT_MODEL, ANTIGRAVITY_LOGIN_HINT, CODEX_ACP, CODEX_CONFIG_ENV,
    CODEX_FULL_ACCESS_MODE, CODEX_PATH_ENV, ClientInfoOverride, DEFAULT_STARTUP_TIMEOUT,
    DEVIN_BYPASS_MODE, DEVIN_ENV_REMOVE, DEVIN_MCP_NOTE, EFFORT_CONFIG_ID, GROK_BUILD_ARGS,
    GROK_BUILD_EFFORT_CONFIG_ID, GROK_BUILD_ENV_REMOVE, HarnessConfig, MINIMAX_CODE_DEFAULT_MODEL,
    MINIMAX_CODE_EFFORT_CONFIG_ID, MINIMAX_CODE_UNUSABLE_MODELS, ModelSelect, OPENCODE_BUILD_MODE,
    PermissionPolicy, SystemPromptStyle,
};
pub(crate) use events::sentence_around_message;
pub use events::{
    AgentError, AgentEvent, AgentInfo, AgentOutput, StartupStep, config_value, effort_option,
    format_startup_steps,
};
pub use handle::{AgentHandle, SpawnOptions, spawn_agent};
pub(crate) use orphan::start_registry as start_process_registry;
pub use permission::{choose_permission, outcome_for};
pub(crate) use process::PRIVATE_ENV;
pub use steering::SteerOutcome;

/// ACP schema types used in this module's public API.
pub use agent_client_protocol::schema::v1 as schema;
