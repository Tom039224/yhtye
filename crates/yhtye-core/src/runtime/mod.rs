//! Multi-session runtime: one loop that owns the state machine
//! ([`crate::domain::Machine`]), every agent session and the MCP server, and
//! publishes an [`crate::api::ApiEvent`] stream (`core-design.md` §8).

mod agent_args;
mod chats;
mod core;
mod driver;
mod emitter;
mod host;
mod launch;
mod orchestration;
mod port;
mod sessions;
mod transcript;

pub use chats::ChatTarget;
pub use core::{Core, CoreConfig};
pub use driver::USER_SESSION;
pub use host::host_name;
pub use orchestration::{OrchError, Orchestration, OrchestrationConfig, UserActionError};
pub use sessions::{agent_ref, session_key};
