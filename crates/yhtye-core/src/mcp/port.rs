//! The seam between the MCP layer and the runtime (`core-design.md` §4).

use async_trait::async_trait;

use super::binding::SessionBinding;
use super::tools::ToolCall;
use crate::domain::ToolError;

/// Executes tool calls. The MCP server has already checked that the caller's
/// role may use the tool and parsed its arguments; the port applies the domain
/// rules and returns the JSON result (sent as `structuredContent` and text).
///
/// Implementations must return promptly (no blocking until another agent acts).
#[async_trait]
pub trait ToolPort: Send + Sync + 'static {
    async fn call(
        &self,
        binding: SessionBinding,
        call: ToolCall,
    ) -> Result<serde_json::Value, ToolError>;
}
