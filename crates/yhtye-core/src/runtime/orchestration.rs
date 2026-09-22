//! Public face of the runtime for one project: one orchestrator session,
//! sub-agent sessions started by the state machine, and the [`ApiEvent`] stream.

use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::driver::{Channels, Cmd, Driver};
use super::emitter::Emitter;
use super::port::LoopPort;
use super::sessions::Sessions;
use crate::acp::{AgentError, HarnessConfig};
use crate::api::{ApiEvent, Snapshot};
use crate::domain::{DomainConfig, Machine, Role, State};
use crate::git::GitService;
use crate::mcp::{McpHost, TokenRegistry};

/// Session key of the orchestrator.
pub const ORCHESTRATOR_SESSION: &str = "orchestrator";

#[derive(Clone)]
pub struct OrchestrationConfig {
    /// Project id used in session bindings and events.
    pub project: String,
    /// Main worktree of the project (the orchestrator's working directory, and
    /// the fallback working directory of sub-agents).
    pub project_dir: PathBuf,
    pub orchestrator: HarnessConfig,
    pub implementer: HarnessConfig,
    pub reviewer: HarnessConfig,
    /// Where the MCP server listens (`127.0.0.1:0`).
    pub mcp_bind: SocketAddr,
    pub domain: DomainConfig,
    /// Branches, worktrees and merges ([`crate::git::NoopGit`] until Stage 3c).
    pub git: Arc<dyn GitService>,
}

impl fmt::Debug for OrchestrationConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OrchestrationConfig")
            .field("project", &self.project)
            .field("project_dir", &self.project_dir)
            .field("mcp_bind", &self.mcp_bind)
            .field("domain", &self.domain)
            .finish_non_exhaustive()
    }
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
    /// Starts the MCP server and the orchestrator session (returns once it is
    /// ready) with an empty state. Events arrive on the returned receiver.
    pub async fn start(
        cfg: OrchestrationConfig,
    ) -> Result<(Self, mpsc::UnboundedReceiver<ApiEvent>), OrchError> {
        let state = State::new(cfg.project.clone(), cfg.domain);
        let (port, tools_rx) = LoopPort::new();
        let (record_tx, records_rx) = mpsc::unbounded_channel();
        let host = McpHost::start(
            cfg.mcp_bind,
            TokenRegistry::new(),
            Arc::new(port),
            Some(record_tx),
        )
        .await?;
        let mcp_addr = host.addr();
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let emit = Emitter::new(&cfg.project, out_tx);
        let (agent_tx, agents_rx) = mpsc::unbounded_channel();
        let cfg = Arc::new(cfg);
        let mut sessions = Sessions::new(cfg.clone(), host, emit.clone(), agent_tx);
        if let Err(e) = sessions.start_orchestrator().await {
            sessions.shutdown().await;
            return Err(e.into());
        }
        let driver = Driver::new(cfg, Machine::new(state), sessions, emit);
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let channels = Channels {
            cmd: cmd_rx,
            tools: tools_rx,
            records: records_rx,
            agents: agents_rx,
        };
        let task = tokio::spawn(driver.run(channels));
        let handle = Self {
            cmd_tx,
            task,
            mcp_addr,
        };
        Ok((handle, out_rx))
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

    /// The current state and the `seq` of the last event it includes.
    pub async fn snapshot(&self) -> Result<Snapshot, OrchError> {
        let (tx, rx) = oneshot::channel();
        self.send(Cmd::Snapshot(tx))?;
        rx.await.map_err(|_| OrchError::Closed)
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
