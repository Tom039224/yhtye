//! Runs `/usage` in a short-lived agent session and caches the result.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use super::{UsageReport, parse_usage_markdown};
use crate::acp::{AgentEvent, AgentOutput, HarnessConfig, SpawnOptions};
use crate::secrets::{Secrets, spawn_agent_with_secrets};

/// The prompt: Claude Code's local usage command (no model call).
const USAGE_COMMAND: &str = "/usage";
/// Upper bound for startup + the command (the adapter itself waits 5 s for the SDK).
const PROBE_TIMEOUT: Duration = Duration::from_secs(90);
/// A cached report younger than this is returned without asking again.
const FRESH_FOR: Duration = Duration::from_secs(60);
/// Even an explicit refresh reuses a report younger than this (one agent start
/// per click is enough; several windows / clients may ask at once).
const MIN_REFRESH: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UsageError {
    #[error("no harness is configured to report usage")]
    NotConfigured,
    #[error("could not start the agent: {0}")]
    Start(String),
    #[error("the agent did not answer /usage in time")]
    Timeout,
    #[error("the agent failed: {0}")]
    Agent(String),
    #[error("the agent's /usage output could not be read")]
    Unreadable,
    #[error("Yhtye is shutting down")]
    Closed,
}

/// Asks the harness once: starts a session in `cwd`, sends `/usage`, reads the
/// reply and stops the session (the process group is reaped by `shutdown`).
pub async fn probe_usage(harness: &HarnessConfig, cwd: &Path) -> Result<UsageReport, UsageError> {
    probe_until(harness, cwd, &Secrets::none(), &CancellationToken::new()).await
}

/// [`probe_usage`] that gives up (stopping the agent) when `cancel` fires.
async fn probe_until(
    harness: &HarnessConfig,
    cwd: &Path,
    secrets: &Secrets,
    cancel: &CancellationToken,
) -> Result<UsageReport, UsageError> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    // Dropping a pending spawn kills the new process group (GroupKillGuard).
    let started = tokio::select! {
        r = tokio::time::timeout(PROBE_TIMEOUT, spawn_agent_with_secrets(secrets, harness, cwd, SpawnOptions::default(), tx)) => r,
        () = cancel.cancelled() => return Err(UsageError::Closed),
    }
    .map_err(|_| UsageError::Timeout)?;
    let handle = started.map_err(|e| UsageError::Start(e.to_string()))?;
    let reply = match handle.prompt_text(USAGE_COMMAND) {
        Ok(()) => tokio::select! {
            r = tokio::time::timeout(PROBE_TIMEOUT, reply_text(&mut rx)) => r.unwrap_or(Err(UsageError::Timeout)),
            () = cancel.cancelled() => Err(UsageError::Closed),
        },
        Err(e) => Err(UsageError::Agent(e.to_string())),
    };
    handle.shutdown().await;
    let text = reply?;
    parse_usage_markdown(&text, now_ms()).ok_or_else(|| {
        tracing::warn!("unreadable /usage output ({} bytes)", text.len());
        UsageError::Unreadable
    })
}

/// The message text of the turn, up to its end.
async fn reply_text(rx: &mut mpsc::UnboundedReceiver<AgentEvent>) -> Result<String, UsageError> {
    let mut text = String::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            AgentEvent::Output(out @ AgentOutput::MessageChunk(_)) => {
                text.push_str(out.chunk_text().unwrap_or_default());
            }
            AgentEvent::TurnEnded(Ok(_)) => return Ok(text),
            AgentEvent::TurnEnded(Err(e)) => return Err(UsageError::Agent(e.to_string())),
            AgentEvent::Exited { code, signal } => {
                return Err(UsageError::Agent(format!(
                    "exited (code {code:?}, signal {signal:?})"
                )));
            }
            _ => {}
        }
    }
    Err(UsageError::Agent("the event channel closed".into()))
}

/// What the service probes and the last outcome.
struct State {
    harness: Option<HarnessConfig>,
    /// Bumped when `harness` changes: a probe that started under an older
    /// number ran the old command, so its report is not kept.
    generation: u64,
    /// The last outcome (a failure too, so waiting callers do not each start
    /// another agent).
    last: Option<(std::time::Instant, Result<UsageReport, UsageError>)>,
}

/// Single-flight, cached access to [`probe_usage`] for the API.
pub struct UsageService {
    cwd: PathBuf,
    secrets: Arc<Secrets>,
    /// Never held across an await, so [`UsageService::set_harness`] does not
    /// wait for a probe.
    state: StdMutex<State>,
    /// Held while probing, so concurrent callers share one probe.
    probing: Mutex<()>,
    /// Fired by [`UsageService::close`]: a running probe stops its agent.
    cancel: CancellationToken,
}

impl UsageService {
    /// `harness: None` disables usage (`get` returns `NotConfigured`). `cwd` is
    /// created on first use (the agent runs there; it writes nothing with the
    /// probe harness's `persistSession: false`).
    #[must_use]
    pub fn new(harness: Option<HarnessConfig>, cwd: PathBuf) -> Self {
        Self {
            cwd,
            secrets: Arc::new(Secrets::none()),
            state: StdMutex::new(State {
                harness,
                generation: 0,
                last: None,
            }),
            probing: Mutex::new(()),
            cancel: CancellationToken::new(),
        }
    }

    /// Gives the probe agent the secret environment variables too.
    #[must_use]
    pub fn with_secrets(mut self, secrets: Arc<Secrets>) -> Self {
        self.secrets = secrets;
        self
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Probes `harness` from now on (the executable it runs was found at
    /// another path) and forgets the last outcome. Nothing changes when it is
    /// the harness already probed. A probe that is running meanwhile is not
    /// waited for, and what it reads is not kept.
    pub fn set_harness(&self, harness: HarnessConfig) {
        let mut state = self.state();
        if state.harness.as_ref() == Some(&harness) {
            return;
        }
        state.harness = Some(harness);
        state.generation += 1;
        state.last = None;
    }

    /// Stops a running probe (its agent is shut down) and refuses new ones.
    /// Waits until no probe is running.
    pub async fn close(&self) {
        self.cancel.cancel();
        drop(self.probing.lock().await);
    }

    /// The latest usage: the cached report if fresh enough, otherwise a new probe.
    pub async fn get(&self, refresh: bool) -> Result<UsageReport, UsageError> {
        let _probing = self.probing.lock().await;
        if self.cancel.is_cancelled() {
            return Err(UsageError::Closed);
        }
        let (harness, generation) = {
            let state = self.state();
            let harness = state.harness.clone().ok_or(UsageError::NotConfigured)?;
            if let Some((at, outcome)) = state.last.as_ref() {
                // Failures are retried after MIN_REFRESH, reports kept for FRESH_FOR.
                let max_age = if refresh || outcome.is_err() {
                    MIN_REFRESH
                } else {
                    FRESH_FOR
                };
                if at.elapsed() < max_age {
                    return outcome.clone();
                }
            }
            (harness, state.generation)
        };
        let outcome = match std::fs::create_dir_all(&self.cwd) {
            Ok(()) => probe_until(&harness, &self.cwd, &self.secrets, &self.cancel).await,
            Err(e) => Err(UsageError::Start(format!(
                "could not create {}: {e}",
                self.cwd.display()
            ))),
        };
        let mut state = self.state();
        if state.generation == generation {
            state.last = Some((std::time::Instant::now(), outcome.clone()));
        }
        outcome
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
