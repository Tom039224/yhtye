//! Public entry point: [`spawn_agent`] and the [`AgentHandle`] it returns.

use std::path::Path;

use agent_client_protocol::schema::v1::{
    ContentBlock, McpServer, SessionConfigOption, SessionId, TextContent,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::config::HarnessConfig;
use super::events::{AgentError, AgentEvent, AgentInfo};
use super::process::spawn_process;
use super::session::{AgentCmd, ReadySlot, TurnState, run_actor};
use super::startup::StartupParams;

/// Options for [`spawn_agent`] besides the harness and working directory.
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    /// MCP servers passed in `session/new` / `session/load`.
    pub mcp_servers: Vec<McpServer>,
    /// Restore this session with `session/load` (if the agent supports it).
    pub resume: Option<SessionId>,
    /// Role system prompt (delivery depends on `HarnessConfig::system_prompt`).
    pub system_prompt: Option<String>,
}

/// Launches the harness in `cwd`, opens one ACP session and returns once it is ready.
///
/// Events (including `Ready`) are sent to `events`; the last event is always
/// `Exited`. Startup failures are returned as errors carrying the failing step,
/// the agent's exit status and the tail of its stderr.
pub async fn spawn_agent(
    harness: &HarnessConfig,
    cwd: &Path,
    options: SpawnOptions,
    events: mpsc::UnboundedSender<AgentEvent>,
) -> Result<AgentHandle, AgentError> {
    let process = spawn_process(harness, cwd, events.clone())?;
    let pid = process.child.id();
    let params = StartupParams {
        harness: harness.clone(),
        cwd: cwd.to_path_buf(),
        mcp: options.mcp_servers,
        resume: options.resume,
        system_prompt: options.system_prompt,
    };
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (ready_tx, ready_rx) = oneshot::channel();
    let turn = TurnState::new();
    let task = tokio::spawn(run_actor(
        process,
        params,
        events,
        cmd_rx,
        turn.clone(),
        ReadySlot::new(ready_tx),
    ));
    let info = ready_rx.await.unwrap_or_else(|_| {
        Err(AgentError::Startup {
            step: "connect".into(),
            message: "the session actor stopped unexpectedly".into(),
        })
    })?;
    Ok(AgentHandle {
        cmd_tx,
        turn,
        info,
        pid,
        task: Some(task),
    })
}

/// Controls one running agent session. Dropping it shuts the agent down
/// (the process group is killed); prefer [`AgentHandle::shutdown`] to wait for it.
pub struct AgentHandle {
    cmd_tx: mpsc::UnboundedSender<AgentCmd>,
    turn: TurnState,
    info: AgentInfo,
    pid: Option<u32>,
    task: Option<JoinHandle<()>>,
}

impl AgentHandle {
    /// Session state at startup (session id, modes, config options, capabilities).
    #[must_use]
    pub fn info(&self) -> &AgentInfo {
        &self.info
    }

    /// OS process id of the agent process (the leader of its process group).
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Starts a turn. The result arrives as `AgentEvent::TurnEnded`.
    /// Fails with [`AgentError::Busy`] while another turn is running.
    pub fn prompt(&self, blocks: Vec<ContentBlock>) -> Result<(), AgentError> {
        if !self.turn.try_begin() {
            return Err(AgentError::Busy);
        }
        self.send(AgentCmd::Prompt(blocks))
            .inspect_err(|_| self.turn.abort_begin())
    }

    /// [`AgentHandle::prompt`] with a single text block.
    pub fn prompt_text(&self, text: impl Into<String>) -> Result<(), AgentError> {
        self.prompt(vec![ContentBlock::Text(TextContent::new(text.into()))])
    }

    /// Sends `session/cancel` if a turn is running; the turn then ends with
    /// `StopReason::Cancelled`. Never waits for the turn.
    pub fn cancel(&self) -> Result<(), AgentError> {
        self.send(AgentCmd::Cancel)
    }

    /// Whether a turn is currently running.
    #[must_use]
    pub fn is_turn_running(&self) -> bool {
        self.turn.is_running()
    }

    /// `session/set_mode`.
    pub async fn set_mode(&self, mode: impl Into<String>) -> Result<(), AgentError> {
        let (reply, rx) = oneshot::channel();
        self.send(AgentCmd::SetMode {
            mode: mode.into(),
            reply,
        })?;
        rx.await.map_err(|_| AgentError::Closed)?
    }

    /// `session/set_config_option`; returns the updated options.
    pub async fn set_config_option(
        &self,
        id: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Vec<SessionConfigOption>, AgentError> {
        let (reply, rx) = oneshot::channel();
        self.send(AgentCmd::SetConfigOption {
            id: id.into(),
            value: value.into(),
            reply,
        })?;
        rx.await.map_err(|_| AgentError::Closed)?
    }

    /// Cancels a running turn (waiting up to 10 s for it to end), closes the
    /// connection and reaps the process group. Returns after `Exited` was emitted.
    pub async fn shutdown(mut self) {
        let _ = self.cmd_tx.send(AgentCmd::Shutdown);
        if let Some(task) = self.task.take()
            && let Err(e) = task.await
        {
            tracing::error!("agent session actor panicked: {e}");
        }
    }

    fn send(&self, cmd: AgentCmd) -> Result<(), AgentError> {
        self.cmd_tx.send(cmd).map_err(|_| AgentError::Closed)
    }
}
