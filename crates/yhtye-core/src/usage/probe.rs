//! Reads the usage from the OAuth endpoint ([`super::oauth`]), or by running
//! `/usage` in a short-lived agent session when that gives nothing, and caches
//! the result.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use super::{OAuthError, OAuthUsage, UsageReport, parse_usage_markdown};
use crate::acp::{AgentEvent, AgentOutput, HarnessConfig, SpawnOptions};
use crate::clock::now_ms;
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
/// How long a 429 without a usable `Retry-After` keeps the endpoint untouched.
const DEFAULT_RATE_LIMIT_WAIT: Duration = Duration::from_secs(300);
/// The adapter's stderr line when its structured `/usage` failed (it then
/// answers with Claude Code's plain text); kept to explain an unreadable reply.
const STRUCTURED_USAGE_MARKER: &str = "Structured /usage";
/// How much of an unreadable reply (or adapter line) a log line carries.
const LOG_SNIPPET_CHARS: usize = 160;
/// Adapter lines kept for the log of one probe.
const MAX_ADAPTER_NOTES: usize = 5;

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
    /// The OAuth usage endpoint answered 429: it is not asked again (nor is the
    /// agent probe started in its place) until `retry_after` has passed.
    #[error("{}", rate_limited_text(*.retry_after))]
    RateLimited { retry_after: Option<Duration> },
    #[error("Yhtye is shutting down")]
    Closed,
}

fn rate_limited_text(retry_after: Option<Duration>) -> String {
    match retry_after {
        Some(wait) => format!(
            "the usage API is rate limited; try again in {} s",
            wait.as_secs().max(1)
        ),
        None => "the usage API is rate limited".into(),
    }
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
    let mut adapter_notes = Vec::new();
    let reply = match handle.prompt_text(USAGE_COMMAND) {
        Ok(()) => tokio::select! {
            r = tokio::time::timeout(PROBE_TIMEOUT, reply_text(&mut rx, &mut adapter_notes)) => r.unwrap_or(Err(UsageError::Timeout)),
            () = cancel.cancelled() => Err(UsageError::Closed),
        },
        Err(e) => Err(UsageError::Agent(e.to_string())),
    };
    handle.shutdown().await;
    let text = reply?;
    parse_usage_markdown(&text, now_ms()).ok_or_else(|| {
        tracing::warn!(
            "unreadable /usage output ({} bytes): {}{}",
            text.len(),
            log_snippet(&text),
            adapter_notes_suffix(&adapter_notes),
        );
        UsageError::Unreadable
    })
}

/// The message text of the turn, up to its end. The adapter's stderr lines
/// that say its structured `/usage` failed are collected into `adapter_notes`
/// (the reply is then Claude Code's plain text, which cannot be read).
async fn reply_text(
    rx: &mut mpsc::UnboundedReceiver<AgentEvent>,
    adapter_notes: &mut Vec<String>,
) -> Result<String, UsageError> {
    let mut text = String::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            AgentEvent::Stderr(line)
                if line.contains(STRUCTURED_USAGE_MARKER)
                    && adapter_notes.len() < MAX_ADAPTER_NOTES =>
            {
                adapter_notes.push(log_snippet(&line));
            }
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

fn adapter_notes_suffix(notes: &[String]) -> String {
    if notes.is_empty() {
        String::new()
    } else {
        format!("; adapter stderr: {}", notes.join(" | "))
    }
}

/// The start of `text` for a log line: identifiers (email addresses, the
/// value of an `Email:` / `Organization:` line) are hidden, newlines are
/// written `\n`, and it is cut at [`LOG_SNIPPET_CHARS`].
fn log_snippet(text: &str) -> String {
    let redacted = text
        .lines()
        .map(redact_line)
        .collect::<Vec<_>>()
        .join("\\n");
    let mut snippet: String = redacted.chars().take(LOG_SNIPPET_CHARS).collect();
    if redacted.chars().count() > LOG_SNIPPET_CHARS {
        snippet.push('…');
    }
    snippet
}

fn redact_line(line: &str) -> String {
    const LABELS: [&str; 4] = ["email", "organization", "account", "login"];
    if let Some((key, _)) = line.split_once(':') {
        let lower = key.trim().to_lowercase();
        if LABELS.iter().any(|l| lower.starts_with(l)) {
            return format!("{key}: <redacted>");
        }
    }
    line.split_inclusive(char::is_whitespace)
        .map(|piece| {
            let word = piece.trim_end();
            if word.contains('@') {
                format!("<email>{}", &piece[word.len()..])
            } else {
                piece.to_string()
            }
        })
        .collect()
}

/// What the service probes and the last outcome.
struct State {
    harness: Option<HarnessConfig>,
    /// Bumped when `harness` changes: a probe that started under an older
    /// number ran the old command, so its report is not kept.
    generation: u64,
    /// The last outcome (a failure too, so waiting callers do not each start
    /// another agent).
    last: Option<(Instant, Result<UsageReport, UsageError>)>,
    /// Set by a 429 of the OAuth endpoint: nothing is fetched before it.
    rate_limited_until: Option<Instant>,
}

/// Single-flight, cached access to the usage for the API: the OAuth endpoint
/// first, [`probe_usage`] when that gives nothing.
pub struct UsageService {
    cwd: PathBuf,
    /// `None`: only the agent probe is used.
    oauth: Option<OAuthUsage>,
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
            oauth: None,
            secrets: Arc::new(Secrets::none()),
            state: StdMutex::new(State {
                harness,
                generation: 0,
                last: None,
                rate_limited_until: None,
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

    /// Reads the usage from the OAuth endpoint first; the agent probe runs
    /// only when that fails (not when it answers 429).
    #[must_use]
    pub fn with_oauth(mut self, oauth: OAuthUsage) -> Self {
        self.oauth = Some(oauth);
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
            if state.harness.is_none() && self.oauth.is_none() {
                return Err(UsageError::NotConfigured);
            }
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
            if let Some(until) = state.rate_limited_until {
                let wait = until.saturating_duration_since(Instant::now());
                if !wait.is_zero() {
                    return Err(UsageError::RateLimited {
                        retry_after: Some(wait),
                    });
                }
            }
            (state.harness.clone(), state.generation)
        };
        let outcome = self.fetch(harness.as_ref()).await;
        let mut state = self.state();
        if state.generation == generation {
            state.last = Some((Instant::now(), outcome.clone()));
        }
        outcome
    }

    /// The OAuth endpoint, then (unless it is rate limited) the agent probe.
    async fn fetch(&self, harness: Option<&HarnessConfig>) -> Result<UsageReport, UsageError> {
        let started = Instant::now();
        if let Some(oauth) = &self.oauth {
            let attempt = tokio::select! {
                r = oauth.fetch(now_ms()) => r,
                () = self.cancel.cancelled() => return Err(UsageError::Closed),
            };
            match attempt {
                Ok(report) => {
                    log_read("oauth", started, &report);
                    return Ok(report);
                }
                Err(OAuthError::RateLimited { retry_after }) => {
                    let wait = retry_after.unwrap_or(DEFAULT_RATE_LIMIT_WAIT);
                    self.state().rate_limited_until = Some(Instant::now() + wait);
                    tracing::warn!(
                        "usage API rate limited; not asked again for {} s",
                        wait.as_secs()
                    );
                    return Err(UsageError::RateLimited {
                        retry_after: Some(wait),
                    });
                }
                Err(e @ OAuthError::NoCredentials(_)) => {
                    tracing::info!("usage API not used ({e}); falling back to /usage");
                }
                Err(e) => tracing::warn!("usage API failed ({e}); falling back to /usage"),
            }
        }
        let harness = harness.ok_or(UsageError::NotConfigured)?;
        let outcome = match std::fs::create_dir_all(&self.cwd) {
            Ok(()) => probe_until(harness, &self.cwd, &self.secrets, &self.cancel).await,
            Err(e) => Err(UsageError::Start(format!(
                "could not create {}: {e}",
                self.cwd.display()
            ))),
        };
        if let Ok(report) = &outcome {
            log_read("acp", started, report);
        }
        outcome
    }
}

fn log_read(source: &str, started: Instant, report: &UsageReport) {
    tracing::info!(
        "usage read via {source} in {} ms ({} windows)",
        started.elapsed().as_millis(),
        report.windows.len()
    );
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1::{ContentBlock, ContentChunk, StopReason, TextContent};

    use super::*;

    #[test]
    fn the_snippet_escapes_newlines_hides_identifiers_and_is_cut() {
        let text = "Login method: Claude Max Account\nEmail: me@example.com\nmail me@example.com now\nCurrent week: 43%";
        let snippet = log_snippet(text);
        assert_eq!(
            snippet,
            "Login method: <redacted>\\nEmail: <redacted>\\nmail <email> now\\nCurrent week: 43%"
        );
        let long = "x".repeat(500);
        let snippet = log_snippet(&long);
        assert_eq!(snippet.chars().count(), LOG_SNIPPET_CHARS + 1);
        assert!(snippet.ends_with('…'));
    }

    #[test]
    fn the_plain_fallback_fixture_gives_a_readable_snippet() {
        let snippet = log_snippet(include_str!("testdata/usage-plain-fallback.txt"));
        assert!(snippet.starts_with("You are currently using your subscription"));
        assert!(!snippet.contains('\n'));
    }

    #[tokio::test]
    async fn only_the_adapters_structured_usage_lines_are_collected() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        for line in [
            "Structured /usage timed out after 5000ms; falling back to the CLI",
            "something else",
            "Structured /usage failed for me@example.com",
        ] {
            tx.send(AgentEvent::Stderr(line.into())).expect("send");
        }
        let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new("hello")));
        tx.send(AgentEvent::Output(AgentOutput::MessageChunk(chunk)))
            .expect("send");
        tx.send(AgentEvent::TurnEnded(Ok(StopReason::EndTurn)))
            .expect("send");
        let mut notes = Vec::new();
        let text = reply_text(&mut rx, &mut notes).await.expect("reply");
        assert_eq!(text, "hello");
        assert_eq!(
            notes,
            [
                "Structured /usage timed out after 5000ms; falling back to the CLI",
                "Structured /usage failed for <email>"
            ]
        );
    }

    #[tokio::test]
    async fn a_pending_rate_limit_blocks_every_fetch_even_after_the_cache_expired() {
        // The endpoint is unreachable: reaching it would be an `Unavailable`
        // fallback, and the missing harness a `NotConfigured`.
        let dir = tempfile::tempdir().expect("tempdir");
        let oauth = OAuthUsage::new()
            .with_url("http://127.0.0.1:9/")
            .with_config_dir(dir.path());
        let service = UsageService::new(None, dir.path().join("probe")).with_oauth(oauth);
        service.state().rate_limited_until = Some(Instant::now() + Duration::from_secs(60));
        for refresh in [false, true] {
            let Err(UsageError::RateLimited { retry_after }) = service.get(refresh).await else {
                panic!("expected RateLimited");
            };
            assert!(retry_after.is_some_and(|w| w <= Duration::from_secs(60)));
        }
        // Once it has passed, the endpoint is asked again (no login here).
        service.state().rate_limited_until = Some(Instant::now());
        assert_eq!(service.get(false).await, Err(UsageError::NotConfigured));
    }
}
