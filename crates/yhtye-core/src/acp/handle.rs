//! Public entry point: [`spawn_agent`] and the [`AgentHandle`] it returns.

use std::future::Future;
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
use super::steering::SteerOutcome;
use crate::secrets::SecretEnv;

/// Options for [`spawn_agent`] besides the harness and working directory.
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    /// MCP servers passed in `session/new` / `session/load`.
    pub mcp_servers: Vec<McpServer>,
    /// Restore this session with `session/load` (if the agent supports it).
    pub resume: Option<SessionId>,
    /// Role system prompt (delivery depends on `HarnessConfig::system_prompt`).
    pub system_prompt: Option<String>,
    /// Secret environment variables added to the agent process (below the
    /// harness's own `env`, which wins on a clash). `Debug` shows names only.
    pub secret_env: SecretEnv,
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
    let note = harness.launch_note.as_deref();
    let process = spawn_process(harness, cwd, &options.secret_env, events.clone())
        .map_err(|e| with_launch_note(e, note))?;
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
    });
    let info = info.map_err(|e| with_launch_note(e, note))?;
    Ok(AgentHandle {
        cmd_tx,
        turn,
        info,
        pid,
        task: Some(task),
    })
}

/// `error` of a launch or startup step with the harness's
/// [`launch_note`](HarnessConfig::launch_note) (which adapter version and
/// executable ran, and how to go back to the verified ones) appended. A
/// timeout becomes a startup error so it can carry the note; without a note
/// every error stays as it is.
fn with_launch_note(error: AgentError, note: Option<&str>) -> AgentError {
    let Some(note) = note else {
        return error;
    };
    match error {
        AgentError::Spawn { command, message } => AgentError::Spawn {
            command,
            message: format!("{message}\n{note}"),
        },
        AgentError::Startup { step, message } => AgentError::Startup {
            step,
            message: format!("{message}\n{note}"),
        },
        AgentError::Timeout { step, millis } => AgentError::Startup {
            step,
            message: format!("timed out after {millis} ms\n{note}"),
        },
        other => other,
    }
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

    /// Whether the agent supports [`AgentHandle::steer`] (it advertises
    /// `_session/steering` in `initialize`).
    #[must_use]
    pub fn supports_steering(&self) -> bool {
        self.info.supports_steering
    }

    /// Injects `blocks` into the running turn (`_session/steering`). Like
    /// [`AgentHandle::prompt`] and [`AgentHandle::cancel`] it never waits for the
    /// turn: the request is queued behind earlier commands now, and the returned
    /// future (not borrowing the handle) resolves to the agent's answer:
    /// - [`SteerOutcome::Injected`]: the turn goes on and ends with one `TurnEnded`.
    /// - [`SteerOutcome::PromptRequired`]: no turn was running (checked before
    ///   sending, and by the agent); nothing happened, so prompt instead.
    /// - [`SteerOutcome::StartedNewTurn`]: the agent ran it as an untracked turn.
    ///
    /// Fails with [`AgentError::SteeringUnsupported`] (without sending) when
    /// the agent does not advertise steering, and with `Request` / `Closed`
    /// when the request fails.
    pub fn steer(
        &self,
        blocks: Vec<ContentBlock>,
    ) -> impl Future<Output = Result<SteerOutcome, AgentError>> + Send + 'static {
        let pending = if self.supports_steering() {
            let (reply, rx) = oneshot::channel();
            self.send(AgentCmd::Steer { blocks, reply }).map(|()| rx)
        } else {
            Err(AgentError::SteeringUnsupported)
        };
        async move { pending?.await.map_err(|_| AgentError::Closed)? }
    }

    /// [`AgentHandle::steer`] with a single text block.
    pub fn steer_text(
        &self,
        text: impl Into<String>,
    ) -> impl Future<Output = Result<SteerOutcome, AgentError>> + Send + 'static {
        self.steer(vec![ContentBlock::Text(TextContent::new(text.into()))])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_note_follows_the_startup_errors_and_a_timeout_becomes_one() {
        let startup = AgentError::Startup {
            step: "initialize".into(),
            message: "boom".into(),
        };
        assert_eq!(
            with_launch_note(startup.clone(), Some("NOTE")),
            AgentError::Startup {
                step: "initialize".into(),
                message: "boom\nNOTE".into()
            }
        );
        assert_eq!(
            with_launch_note(
                AgentError::Spawn {
                    command: "npx".into(),
                    message: "no such file".into()
                },
                Some("NOTE")
            ),
            AgentError::Spawn {
                command: "npx".into(),
                message: "no such file\nNOTE".into()
            }
        );
        let timeout = AgentError::Timeout {
            step: "session/new".into(),
            millis: 1500,
        };
        assert_eq!(
            with_launch_note(timeout.clone(), Some("NOTE")),
            AgentError::Startup {
                step: "session/new".into(),
                message: "timed out after 1500 ms\nNOTE".into()
            }
        );
        // No note: nothing changes. Errors that are not about launching stay.
        assert_eq!(with_launch_note(timeout.clone(), None), timeout);
        assert_eq!(with_launch_note(startup.clone(), None), startup);
        assert_eq!(
            with_launch_note(AgentError::Closed, Some("NOTE")),
            AgentError::Closed
        );
    }
}
