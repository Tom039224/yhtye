//! Public face of the Stage 2 multi-session runtime: one orchestrator session,
//! sub-agent sessions started on demand by tool calls, and the orchestrator inbox.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::driver::{Cmd, Driver};
use super::memory_port::MemoryToolPort;
use crate::acp::{AgentError, AgentEvent, HarnessConfig};
use crate::domain::Role;
use crate::mcp::{McpHost, TokenRegistry, ToolCallRecord};

/// Session key of the orchestrator.
pub const ORCHESTRATOR_SESSION: &str = "orchestrator";

#[derive(Debug, Clone)]
pub struct OrchestrationConfig {
    /// Project id used in session bindings.
    pub project: String,
    /// Working directory of the orchestrator. In Stage 2 sub-agents also run here
    /// (per-task worktrees arrive with the git module in Stage 3).
    pub project_dir: PathBuf,
    /// Reported as `base_branch` by `create_group`.
    pub base_branch: String,
    pub orchestrator: HarnessConfig,
    pub implementer: HarnessConfig,
    pub reviewer: HarnessConfig,
    /// Where the MCP server listens (`127.0.0.1:0`).
    pub mcp_bind: SocketAddr,
}

impl OrchestrationConfig {
    #[must_use]
    pub fn harness(&self, role: Role) -> &HarnessConfig {
        match role {
            Role::Orchestrator => &self.orchestrator,
            Role::Implementer => &self.implementer,
            Role::Reviewer => &self.reviewer,
        }
    }
}

/// What happens in the runtime, in order (for the UI later; for tests now).
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchEvent {
    SessionStarted {
        session: String,
        role: Role,
        task: Option<String>,
        pid: Option<u32>,
    },
    SessionFailed {
        session: String,
        error: String,
    },
    /// The session was shut down (or its process exited) and its token revoked.
    SessionStopped {
        session: String,
    },
    Agent {
        session: String,
        event: AgentEvent,
    },
    ToolCalled(ToolCallRecord),
    /// A prompt was sent to a session (for the orchestrator: an inbox batch).
    Prompted {
        session: String,
        text: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum OrchError {
    #[error("MCP server failed to start: {0}")]
    Mcp(#[from] std::io::Error),
    #[error("orchestrator session failed to start: {0}")]
    Orchestrator(#[from] AgentError),
    #[error("the orchestration loop has stopped")]
    Closed,
}

/// Handle to a running orchestration. Call [`Orchestration::shutdown`] to stop
/// every agent and the MCP server.
pub struct Orchestration {
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    task: JoinHandle<()>,
    mcp_addr: SocketAddr,
}

impl Orchestration {
    /// Starts the MCP server and the orchestrator session (returns once it is ready).
    pub async fn start(
        cfg: OrchestrationConfig,
    ) -> Result<(Self, mpsc::UnboundedReceiver<OrchEvent>), OrchError> {
        let (port, effects_rx) = MemoryToolPort::new(&cfg.base_branch);
        let (tool_tx, tool_rx) = mpsc::unbounded_channel();
        let host = McpHost::start(
            cfg.mcp_bind,
            TokenRegistry::new(),
            Arc::new(port.clone()),
            Some(tool_tx),
        )
        .await?;
        let mcp_addr = host.addr();
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let mut driver = Driver::new(Arc::new(cfg), host, port, out_tx);
        if let Err(e) = driver.start_orchestrator().await {
            driver.shutdown().await;
            return Err(e.into());
        }
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(driver.run(cmd_rx, effects_rx, tool_rx));
        Ok((
            Self {
                cmd_tx,
                task,
                mcp_addr,
            },
            out_rx,
        ))
    }

    /// Address of the MCP server.
    #[must_use]
    pub fn mcp_addr(&self) -> SocketAddr {
        self.mcp_addr
    }

    /// Queues a `user_message` for the orchestrator (sent when it is idle).
    pub fn send_user_message(&self, text: impl Into<String>) -> Result<(), OrchError> {
        self.send(Cmd::UserMessage(text.into()))
    }

    /// Cancels the orchestrator's running turn, if any.
    pub fn cancel_orchestrator_turn(&self) -> Result<(), OrchError> {
        self.send(Cmd::CancelOrchestrator)
    }

    /// Stops all agent sessions (reaping their processes) and the MCP server.
    pub async fn shutdown(self) {
        let (done_tx, done_rx) = oneshot::channel();
        if self.cmd_tx.send(Cmd::Shutdown(done_tx)).is_ok() {
            let _ = done_rx.await;
        }
        if let Err(e) = self.task.await {
            tracing::error!("orchestration loop panicked: {e}");
        }
    }

    fn send(&self, cmd: Cmd) -> Result<(), OrchError> {
        self.cmd_tx.send(cmd).map_err(|_| OrchError::Closed)
    }
}
