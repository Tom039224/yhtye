//! Yhtye core library (Tauri-independent). See `docs/architecture/core-design.md`.
//!
//! - [`acp`]: ACP client (agent processes and sessions)
//! - [`mcp`]: the Yhtye MCP server agents call back into
//! - [`domain`]: the orchestration state machine (pure)
//! - [`git`]: git operations behind a trait
//! - [`api`]: the UI-facing event stream and snapshot
//! - [`prompts`]: role system prompts
//! - [`store`]: SQLite persistence (event log + current-state tables)
//! - [`runtime`]: the loop that ties sessions, MCP, git and the state machine together

pub mod acp;
pub mod api;
pub mod domain;
pub mod git;
pub mod mcp;
pub mod prompts;
pub mod runtime;
pub mod store;
