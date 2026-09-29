//! Yhtye core library (Tauri-independent). See `docs/architecture/core-design.md`.
//!
//! - [`acp`]: ACP client (agent processes and sessions)
//! - [`agents`]: which harness × model runs each role (settings, registry, model lists)
//! - [`mcp`]: the Yhtye MCP server agents call back into
//! - [`domain`]: the orchestration state machine (pure)
//! - [`git`]: git operations behind a trait
//! - [`api`]: the UI-facing event stream and snapshot
//! - [`prompts`]: role system prompts
//! - [`store`]: SQLite persistence (event log + current-state tables)
//! - [`runtime`]: the loop that ties sessions, MCP, git and the state machine together
//! - [`secrets`]: secret environment variables kept in the OS keyring
//! - [`usage`]: subscription usage / quota read from the harness

pub mod acp;
pub mod agents;
pub mod api;
pub mod domain;
pub mod git;
pub mod mcp;
pub mod prompts;
pub mod runtime;
pub mod secrets;
pub mod store;
pub mod usage;
