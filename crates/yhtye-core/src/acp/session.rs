//! The session actor: one agent process = one ACP connection = one tokio task.
//!
//! The command loop never awaits a prompt response: each turn runs in its own
//! connection task, so `Cancel` reaches the agent (`session/cancel`) mid-turn.
//! Handlers answer immediately and never block the dispatch loop.

use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, PromptRequest, RequestPermissionRequest,
    RequestPermissionResponse, SessionConfigOption, SessionId, StopReason, TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use super::events::{AgentError, AgentEvent, AgentInfo, AgentOutput};
use super::permission::{choose_permission, outcome_for};
use super::process::{self, AgentProcess};
use super::startup::{self, StartupParams};

/// How long `Shutdown` waits for a cancelled turn to end.
const SHUTDOWN_CANCEL_WAIT: Duration = Duration::from_secs(10);
/// Timeout for `set_mode` / `set_config_option` after startup.
const RUNTIME_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Commands from [`super::AgentHandle`] to the actor.
pub(crate) enum AgentCmd {
    Prompt(Vec<ContentBlock>),
    Cancel,
    SetMode {
        mode: String,
        reply: oneshot::Sender<Result<(), AgentError>>,
    },
    SetConfigOption {
        id: String,
        value: String,
        reply: oneshot::Sender<Result<Vec<SessionConfigOption>, AgentError>>,
    },
    Shutdown,
}

/// `session/update` received untyped so that unknown update kinds are kept.
#[derive(Debug, Clone, Serialize, Deserialize, agent_client_protocol::JsonRpcNotification)]
#[notification(method = "session/update")]
struct RawSessionUpdate(serde_json::Value);

/// Whether a turn is running. Exactly one party flips it back to `false` and
/// emits `TurnEnded` for that turn.
#[derive(Clone)]
pub(crate) struct TurnState(Arc<watch::Sender<bool>>);

impl TurnState {
    pub fn new() -> Self {
        Self(Arc::new(watch::Sender::new(false)))
    }

    /// Marks a turn as started; `false` if one is already running.
    pub fn try_begin(&self) -> bool {
        self.0
            .send_if_modified(|running| !std::mem::replace(running, true))
    }

    /// Undoes [`TurnState::try_begin`] when the prompt could not be delivered.
    pub fn abort_begin(&self) {
        self.0.send_replace(false);
    }

    /// Ends the running turn, emitting `TurnEnded` — unless it was already ended.
    fn end(
        &self,
        events: &mpsc::UnboundedSender<AgentEvent>,
        result: Result<StopReason, AgentError>,
    ) {
        if self
            .0
            .send_if_modified(|running| std::mem::replace(running, false))
        {
            let _ = events.send(AgentEvent::TurnEnded(result));
        }
    }

    pub fn is_running(&self) -> bool {
        *self.0.borrow()
    }

    async fn wait_idle(&self) {
        let mut rx = self.0.subscribe();
        let _ = rx.wait_for(|running| !running).await;
    }
}

/// Startup result slot: filled with the error if startup fails.
pub(crate) struct ReadySlot {
    tx: Option<oneshot::Sender<Result<AgentInfo, AgentError>>>,
    error: Option<AgentError>,
}

impl ReadySlot {
    pub fn new(tx: oneshot::Sender<Result<AgentInfo, AgentError>>) -> Self {
        Self {
            tx: Some(tx),
            error: None,
        }
    }
}

/// Runs the whole life of one agent process. Always ends with `AgentEvent::Exited`.
pub(crate) async fn run_actor(
    process: AgentProcess,
    params: StartupParams,
    events: mpsc::UnboundedSender<AgentEvent>,
    cmds: mpsc::UnboundedReceiver<AgentCmd>,
    turn: TurnState,
    mut ready: ReadySlot,
) {
    let AgentProcess {
        mut child,
        stdin,
        stdout,
        mut group,
        stderr_tail,
        stderr_task,
    } = process;
    let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
    let result = connection(transport, &params, &events, cmds, &turn, &mut ready).await;
    if let Err(e) = &result {
        tracing::warn!("agent connection ended with error: {e}");
    }
    turn.end(&events, Err(AgentError::Closed));

    let status = process::terminate(&mut child, &mut group).await;
    let _ = tokio::time::timeout(Duration::from_millis(500), stderr_task).await;
    let (code, signal) = process::exit_parts(status);
    if let Some(tx) = ready.tx.take() {
        let error = ready.error.take().unwrap_or_else(|| AgentError::Startup {
            step: "connect".into(),
            message: result
                .err()
                .map_or_else(|| "connection closed".into(), |e| e.to_string()),
        });
        let _ = tx.send(Err(annotate_startup_error(
            error,
            code,
            signal,
            &stderr_tail,
        )));
    }
    let _ = events.send(AgentEvent::Exited { code, signal });
}

fn annotate_startup_error(
    error: AgentError,
    code: Option<i32>,
    signal: Option<i32>,
    stderr: &process::StderrTail,
) -> AgentError {
    let exit = match (code, signal) {
        (Some(c), _) => format!(" (agent exited with code {c})"),
        (None, Some(s)) => format!(" (agent killed by signal {s})"),
        _ => String::new(),
    };
    match error {
        AgentError::Startup { step, message } => AgentError::Startup {
            step,
            message: stderr.annotate(format!("{message}{exit}")),
        },
        other => other,
    }
}

async fn connection(
    transport: impl agent_client_protocol::ConnectTo<Client> + 'static,
    params: &StartupParams,
    events: &mpsc::UnboundedSender<AgentEvent>,
    cmds: mpsc::UnboundedReceiver<AgentCmd>,
    turn: &TurnState,
    ready: &mut ReadySlot,
) -> Result<(), agent_client_protocol::Error> {
    let update_events = events.clone();
    let permission_events = events.clone();
    Client
        .builder()
        .name("yhtye")
        .on_receive_notification(
            async move |n: RawSessionUpdate, _cx| {
                let update = n.0.get("update").cloned().unwrap_or_default();
                let _ =
                    update_events.send(AgentEvent::Output(AgentOutput::from_update_json(update)));
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, _cx| {
                let choice = choose_permission(&req.options);
                let chosen = choice.map(|o| o.kind);
                let outcome = outcome_for(choice);
                let _ = permission_events.send(AgentEvent::PermissionAutoAnswered {
                    tool_call: Box::new(req.tool_call),
                    options: req.options,
                    chosen,
                });
                responder.respond(RequestPermissionResponse::new(outcome))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async |cx: ConnectionTo<Agent>| {
            let info = match startup::open_session(&cx, params).await {
                Ok(info) => info,
                Err(e) => {
                    ready.error = Some(e);
                    return Ok(());
                }
            };
            let _ = events.send(AgentEvent::Ready(Box::new(info.clone())));
            let preamble = params.first_prompt_preamble(info.resumed);
            let session = info.acp_session_id.clone();
            if let Some(tx) = ready.tx.take() {
                let _ = tx.send(Ok(info));
            }
            let mut lp = CommandLoop {
                cx,
                session,
                events: events.clone(),
                turn: turn.clone(),
                preamble,
            };
            lp.run(cmds).await;
            Ok(())
        })
        .await
}

struct CommandLoop {
    cx: ConnectionTo<Agent>,
    session: SessionId,
    events: mpsc::UnboundedSender<AgentEvent>,
    turn: TurnState,
    /// System prompt still to be prepended to the first prompt (FirstPrompt style).
    preamble: Option<String>,
}

impl CommandLoop {
    async fn run(&mut self, mut cmds: mpsc::UnboundedReceiver<AgentCmd>) {
        loop {
            let cmd = tokio::select! {
                cmd = cmds.recv() => cmd,
                () = self.cx.incoming_closed() => return,
            };
            match cmd {
                None | Some(AgentCmd::Shutdown) => return self.shutdown().await,
                Some(AgentCmd::Prompt(blocks)) => self.start_turn(blocks),
                Some(AgentCmd::Cancel) => self.cancel(),
                Some(AgentCmd::SetMode { mode, reply }) => self.set_mode(mode, reply),
                Some(AgentCmd::SetConfigOption { id, value, reply }) => {
                    self.set_config_option(id, value, reply);
                }
            }
        }
    }

    /// Sends the prompt from a connection task; the loop keeps serving commands.
    fn start_turn(&mut self, mut blocks: Vec<ContentBlock>) {
        if let Some(preamble) = self.preamble.take() {
            blocks.insert(0, ContentBlock::Text(TextContent::new(preamble)));
        }
        let cx = self.cx.clone();
        let events = self.events.clone();
        let turn = self.turn.clone();
        let request = PromptRequest::new(self.session.clone(), blocks);
        let spawned = self.cx.spawn(async move {
            let result = cx.send_request(request).block_task().await;
            let result = result
                .map(|r| r.stop_reason)
                .map_err(|e| AgentError::Request {
                    method: "session/prompt".into(),
                    message: e.to_string(),
                });
            turn.end(&events, result);
            Ok(())
        });
        if spawned.is_err() {
            self.turn.end(&self.events, Err(AgentError::Closed));
        }
    }

    fn cancel(&self) {
        if !self.turn.is_running() {
            return;
        }
        let note = CancelNotification::new(self.session.clone());
        if let Err(e) = self.cx.send_notification(note) {
            tracing::warn!("failed to send session/cancel: {e}");
        }
    }

    fn set_mode(&self, mode: String, reply: oneshot::Sender<Result<(), AgentError>>) {
        let (cx, session) = (self.cx.clone(), self.session.clone());
        self.spawn_or_close(reply, async move {
            startup::request_set_mode(&cx, &session, &mode, RUNTIME_REQUEST_TIMEOUT)
                .await
                .map_err(as_request_error)
        });
    }

    fn set_config_option(
        &self,
        id: String,
        value: String,
        reply: oneshot::Sender<Result<Vec<SessionConfigOption>, AgentError>>,
    ) {
        let (cx, session) = (self.cx.clone(), self.session.clone());
        self.spawn_or_close(reply, async move {
            startup::set_config_option(&cx, &session, &id, &value, RUNTIME_REQUEST_TIMEOUT)
                .await
                .map_err(as_request_error)
        });
    }

    /// Runs a request off the loop and sends its result to `reply`.
    fn spawn_or_close<T: Send + 'static>(
        &self,
        reply: oneshot::Sender<Result<T, AgentError>>,
        fut: impl Future<Output = Result<T, AgentError>> + Send + 'static,
    ) {
        // If the task cannot be spawned, `reply` is dropped with it and the
        // handle observes `Closed`.
        let _ = self.cx.spawn(async move {
            let _ = reply.send(fut.await);
            Ok(())
        });
    }

    async fn shutdown(&self) {
        if self.turn.is_running() {
            self.cancel();
            if tokio::time::timeout(SHUTDOWN_CANCEL_WAIT, self.turn.wait_idle())
                .await
                .is_err()
            {
                tracing::warn!("turn did not end within {SHUTDOWN_CANCEL_WAIT:?} after cancel");
            }
        }
    }
}

/// Startup-step errors reported by runtime requests are plain request errors.
fn as_request_error(e: AgentError) -> AgentError {
    match e {
        AgentError::Startup { step, message } => AgentError::Request {
            method: step,
            message,
        },
        AgentError::Timeout { step, millis } => AgentError::Request {
            method: step,
            message: format!("timed out after {millis} ms"),
        },
        other => other,
    }
}
