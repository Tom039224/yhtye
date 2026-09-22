//! Yhtye core library (Tauri-independent). See `docs/architecture/core-design.md`.
//!
//! - [`acp`]: ACP client (agent processes and sessions)
//! - [`mcp`]: the Yhtye MCP server agents call back into
//! - [`domain`]: shared vocabulary and pure rules
//! - [`prompts`]: role system prompts
//! - [`runtime`]: multi-session orchestration (Stage 2: in-memory board as the tool port)

pub mod acp;
pub mod domain;
pub mod mcp;
pub mod prompts;
pub mod runtime;
