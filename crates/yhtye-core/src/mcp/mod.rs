//! Yhtye MCP server: the only way agents change Yhtye's state
//! (`docs/architecture/mcp-tools.md`, `core-design.md` §4).

mod binding;
mod port;
mod server;
pub mod tools;

pub use binding::{McpToken, SessionBinding, TokenRegistry};
pub use port::ToolPort;
pub use server::{MCP_SERVER_NAME, McpHost, ToolCallRecord};
pub use tools::{ToolCall, ToolName, tools_for};
