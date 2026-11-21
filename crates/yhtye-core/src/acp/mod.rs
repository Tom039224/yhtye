//! ACP client: launches a harness process, opens one session on it and turns
//! everything the agent reports into typed [`AgentEvent`]s.
//!
//! See `docs/architecture/core-design.md` §3 and `docs/architecture/acp-harnesses.md`.

mod config;
mod events;
mod handle;
#[cfg(all(test, target_os = "linux"))]
mod live_tests;
#[cfg(unix)]
mod orphan;
#[cfg(unix)]
mod parent_death;
mod permission;
mod process;
mod session;
mod startup;
mod steering;

pub use config::{
    ANTHROPIC_API_KEY_ENV, ANTIGRAVITY_DEFAULT_MODEL, ANTIGRAVITY_LOGIN_HINT, CLAUDE_AGENT_ACP,
    CLAUDE_AGENT_ACP_BIN, CLAUDE_AGENT_ACP_PACKAGE, CLAUDE_AGENT_ACP_VERIFIED,
    CLAUDE_API_KEY_SECRET, CLAUDE_CODE_API_ENV_REMOVE, CLAUDE_CODE_EXECUTABLE_ENV,
    CLAUDE_CODE_SUBSCRIPTION_ENV_REMOVE, CODEX_ACP, CODEX_ACP_BIN, CODEX_ACP_PACKAGE,
    CODEX_ACP_VERIFIED, CODEX_CONFIG_ENV, CODEX_FULL_ACCESS_MODE, CODEX_PATH_ENV,
    ClientInfoOverride, DEFAULT_STARTUP_TIMEOUT, DEVIN_BYPASS_MODE, DEVIN_ENV_REMOVE,
    DEVIN_MCP_NOTE, EFFORT_CONFIG_ID, GROK_BUILD_ARGS, GROK_BUILD_EFFORT_CONFIG_ID,
    GROK_BUILD_ENV_REMOVE, HARNESS_ONLY_SECRETS, HarnessConfig, MINIMAX_CODE_DEFAULT_MODEL,
    MINIMAX_CODE_EFFORT_CONFIG_ID, MINIMAX_CODE_UNUSABLE_MODELS, ModelSelect, OPENCODE_BUILD_MODE,
    PI_ACP, PI_ACP_MODEL_ENV, PI_ACP_QUIET_STARTUP_ENV, PI_EFFORT_CONFIG_ID, PI_LOGIN_HINT,
    PI_MCP_NOTE, PermissionPolicy, SecretBinding, SystemPromptStyle,
};
pub use events::{AgentError, AgentEvent, AgentInfo, AgentOutput, config_value, effort_option};
pub use handle::{AgentHandle, SpawnOptions, spawn_agent};
#[cfg(unix)]
pub(crate) use orphan::start_registry as start_process_registry;
pub use permission::{choose_permission, outcome_for};
pub(crate) use process::PRIVATE_ENV;
pub use steering::SteerOutcome;

/// Nothing to sweep at start on Windows: an agent's tree is a Job Object that the
/// OS ends when Yhtye's handle to it closes, however Yhtye dies, so no agent
/// survives it and there is no on-disk record of groups either.
#[cfg(not(unix))]
pub(crate) async fn start_process_registry(_data_dir: &std::path::Path) {}

/// ACP schema types used in this module's public API.
pub use agent_client_protocol::schema::v1 as schema;
