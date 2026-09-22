//! Multi-session runtime: one loop that owns the state machine
//! ([`crate::domain::Machine`]), every agent session and the MCP server, and
//! publishes an [`crate::api::ApiEvent`] stream (`core-design.md` §8).

mod driver;
mod emitter;
mod orchestration;
mod port;
mod sessions;

pub use orchestration::{ORCHESTRATOR_SESSION, OrchError, Orchestration, OrchestrationConfig};
pub use sessions::{agent_ref, session_key};
