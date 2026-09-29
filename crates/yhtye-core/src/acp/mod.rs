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
    DEFAULT_STARTUP_TIMEOUT, EFFORT_CONFIG_ID, HarnessConfig, ModelSelect, OPENCODE_BUILD_MODE,
    ORCHESTRATOR_BUILTIN_TOOLS, SystemPromptStyle,
};
pub use events::{AgentError, AgentEvent, AgentInfo, AgentOutput, config_value};
pub use handle::{AgentHandle, SpawnOptions, spawn_agent};
pub use permission::{choose_permission, outcome_for};

/// ACP schema types used in this module's public API.
pub use agent_client_protocol::schema::v1 as schema;
