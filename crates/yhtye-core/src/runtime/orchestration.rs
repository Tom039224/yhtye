//! Public face of the runtime for one project: one orchestrator session,
//! sub-agent sessions started by the state machine, and the [`ApiEvent`] stream.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;

use super::driver::{Channels, Cmd, Driver, Restarted};
use super::emitter::{Emitter, Publisher};
use super::port::{LoopPort, ToolRequest};
use super::sessions::{Sessions, StoredSession};
use crate::acp::AgentError;
use crate::agents::AgentCatalog;
use crate::api::{ApiEvent, ApiEventBody, Snapshot};
use crate::domain::{DomainConfig, OrchestratorResume, State, ToolError};
use crate::git::GitService;
use crate::mcp::tools::{CancelGroupArgs, CancelTaskArgs};
use crate::mcp::{McpHost, TokenRegistry, ToolCall, ToolCallRecord};
use crate::store::{SessionRecord, Store, StoreError, StoredEvent};

/// Session key of the orchestrator.
pub const ORCHESTRATOR_SESSION: &str = "orchestrator";

#[derive(Clone)]
pub struct OrchestrationConfig {
    /// Project id used in session bindings and events.
    pub project: String,
    /// Main worktree of the project (the orchestrator's working directory, and
    /// the fallback working directory of sub-agents).
    pub project_dir: PathBuf,
    /// Registered harnesses and the harness × model settings of each role
    /// (`core-design.md` §15; [`AgentCatalog::fixed`] for one fixed harness).
    pub agents: Arc<AgentCatalog>,
    /// Where the MCP server listens (`127.0.0.1:0`).
    pub mcp_bind: SocketAddr,
    pub domain: DomainConfig,
    /// Branches, worktrees and merges. The app uses [`crate::git::GitCli`] with
    /// its worktrees under the data directory ([`crate::git::worktree_root`]);
    /// [`crate::git::NoopGit`] is for tests without a repository.
    pub git: Arc<dyn GitService>,
    /// SQLite database (created if missing). The app uses
    /// [`crate::store::db_path`] of its data directory.
    pub db_path: PathBuf,
}

impl fmt::Debug for OrchestrationConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OrchestrationConfig")
            .field("project", &self.project)
            .field("project_dir", &self.project_dir)
            .field("mcp_bind", &self.mcp_bind)
            .field("domain", &self.domain)
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OrchError {
    #[error("MCP server failed to start: {0}")]
    Mcp(#[from] std::io::Error),
    #[error("orchestrator session failed to start: {0}")]
    Orchestrator(#[from] AgentError),
    #[error("database: {0}")]
    Store(#[from] StoreError),
    #[error("the orchestration loop has stopped")]
    Closed,
}

/// Why a user action (cancelling a task or group) failed.
#[derive(Debug, thiserror::Error)]
pub enum UserActionError {
    /// Refused by the state machine (unknown id, wrong state, ...).
    #[error("{0}")]
    Rejected(#[from] ToolError),
    #[error(transparent)]
    Orchestration(#[from] OrchError),
}

/// Handle to a running orchestration. Call [`Orchestration::shutdown`] to stop
/// every agent and the MCP server.
pub struct Orchestration {
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    task: Mutex<Option<JoinHandle<()>>>,
    mcp_addr: SocketAddr,
    store: Store,
    project: String,
}

/// The stored state of a project, and what its sessions were doing.
struct Stored {
    state: State,
    restarted: bool,
    sessions: Vec<SessionRecord>,
}

impl Stored {
    /// Reports every session that was running (or suspended) when Yhtye stopped
    /// as interrupted; returns their ACP session ids to restore, by session key.
    fn interrupt_sessions(&self, emit: &Emitter) -> HashMap<String, StoredSession> {
        for s in &self.sessions {
            emit.send(ApiEventBody::SessionInterrupted {
                session: s.session_key.clone(),
            });
        }
        self.sessions
            .iter()
            .map(|s| {
                let stored = StoredSession {
                    acp_session_id: s.acp_session_id.clone(),
                    agent: s.agent.clone(),
                };
                (s.session_key.clone(), stored)
            })
            .collect()
    }
}

type McpParts = (
    McpHost,
    mpsc::UnboundedReceiver<ToolRequest>,
    mpsc::UnboundedReceiver<ToolCallRecord>,
);

/// Starts the MCP server; tool calls and their records go to the loop.
async fn start_mcp(cfg: &OrchestrationConfig) -> std::io::Result<McpParts> {
    let (port, tools_rx) = LoopPort::new();
    let (record_tx, records_rx) = mpsc::unbounded_channel();
    let host = McpHost::start(
        cfg.mcp_bind,
        TokenRegistry::new(),
        Arc::new(port),
        Some(record_tx),
    )
    .await?;
    Ok((host, tools_rx, records_rx))
}

async fn open_project(store: &Store, cfg: &OrchestrationConfig) -> Result<Stored, StoreError> {
    let (state, restarted) = match store.load_state(&cfg.project).await? {
        Some(mut state) => {
            state.config = cfg.domain; // settings, not history
            (state, true)
        }
        None => {
            let state = State::new(cfg.project.clone(), cfg.domain);
            store.create_project(&state, &cfg.project_dir).await?;
            (state, false)
        }
    };
    let sessions = store
        .sessions(&cfg.project)
        .await?
        .into_iter()
        .filter(|s| s.status.is_resumable())
        .collect();
    Ok(Stored {
        state,
        restarted,
        sessions,
    })
}

impl Orchestration {
    /// Opens the project's stored state (or creates it), starts the MCP server
    /// and the orchestrator session (returns once it is ready). After a restart,
    /// in-flight tasks become `interrupted` and are resumed, restoring their
    /// agent sessions with `session/load` where possible. Events arrive on the
    /// returned receiver.
    pub async fn start(
        cfg: OrchestrationConfig,
    ) -> Result<(Self, mpsc::UnboundedReceiver<ApiEvent>), OrchError> {
        let store = Store::open(&cfg.db_path).await?;
        let stored = open_project(&store, &cfg).await?;
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let publisher = Publisher::new(&cfg.project, store.clone(), out_tx).await?;
        let emit = publisher.emitter();
        let resume = stored.interrupt_sessions(&emit);
        let (host, tools_rx, records_rx) = start_mcp(&cfg).await?;
        let mcp_addr = host.addr();
        let (agent_tx, agents_rx) = mpsc::unbounded_channel();
        let project = cfg.project.clone();
        let cfg = Arc::new(cfg);
        let mut sessions = Sessions::new(cfg.clone(), host, emit, agent_tx, resume);
        let started = match sessions.start_orchestrator().await {
            Ok(s) => s,
            Err(e) => {
                sessions.shutdown().await;
                return Err(e.into());
            }
        };
        let restarted = stored.restarted.then(|| Restarted {
            orchestrator: OrchestratorResume {
                had_session: started.had_session,
                restored: started.restored,
                turn_was_running: orchestrator_turn_running(&stored.sessions),
            },
        });
        let driver = Driver::new(cfg, stored.state, sessions, publisher);
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let channels = Channels {
            cmd: cmd_rx,
            tools: tools_rx,
            records: records_rx,
            agents: agents_rx,
        };
        let task = tokio::spawn(driver.run(channels, restarted));
        let handle = Self {
            cmd_tx,
            task: Mutex::new(Some(task)),
            mcp_addr,
            store,
            project,
        };
        Ok((handle, out_rx))
    }

    /// The database of this orchestration (history, stored state).
    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Stored (durable) events with `seq > after_seq`, oldest first.
    pub async fn events(&self, after_seq: u64, limit: u32) -> Result<Vec<StoredEvent>, OrchError> {
        Ok(self
            .store
            .events_after(&self.project, after_seq, limit)
            .await?)
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
        Ok(rx.await.map_err(|_| OrchError::Closed)??)
    }

    /// Cancels a task as the user (`cancel_task` with the orchestrator's rules);
    /// the orchestrator gets a `user_message` saying so.
    pub async fn cancel_task(
        &self,
        task: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<Value, UserActionError> {
        let call = ToolCall::CancelTask(CancelTaskArgs {
            task_id: task.into(),
            reason: reason.into(),
        });
        self.user_tool(call).await
    }

    /// Cancels a group as the user (`cancel_group`); the orchestrator is told.
    pub async fn cancel_group(
        &self,
        group: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<Value, UserActionError> {
        let call = ToolCall::CancelGroup(CancelGroupArgs {
            group_id: group.into(),
            reason: reason.into(),
        });
        self.user_tool(call).await
    }

    /// Retries the base merge of a `merge_blocked` group (after the user made
    /// the main worktree mergeable). The result is in the group's status and
    /// is also sent to the orchestrator as `merge_result`.
    pub async fn retry_group_merge(
        &self,
        group: impl Into<String>,
    ) -> Result<Value, UserActionError> {
        let (tx, rx) = oneshot::channel();
        self.send(Cmd::RetryGroupMerge(group.into(), tx))?;
        Ok(rx.await.map_err(|_| OrchError::Closed)??)
    }

    async fn user_tool(&self, call: ToolCall) -> Result<Value, UserActionError> {
        let (tx, rx) = oneshot::channel();
        self.send(Cmd::UserTool(call, tx))?;
        Ok(rx.await.map_err(|_| OrchError::Closed)??)
    }

    /// Stops all agent sessions (reaping their processes) and the MCP server.
    /// Later calls (and every other method afterwards) find the loop closed.
    pub async fn shutdown(&self) {
        let (done_tx, done_rx) = oneshot::channel();
        if self.cmd_tx.send(Cmd::Shutdown(done_tx)).is_ok() {
            let _ = done_rx.await;
        }
        let Some(task) = self.task.lock().await.take() else {
            return;
        };
        if let Err(e) = task.await {
            tracing::error!("orchestration loop panicked: {e}");
        }
    }

    fn send(&self, cmd: Cmd) -> Result<(), OrchError> {
        self.cmd_tx.send(cmd).map_err(|_| OrchError::Closed)
    }
}

fn orchestrator_turn_running(sessions: &[SessionRecord]) -> bool {
    sessions
        .iter()
        .any(|s| s.session_key == ORCHESTRATOR_SESSION && s.turn_running)
}
