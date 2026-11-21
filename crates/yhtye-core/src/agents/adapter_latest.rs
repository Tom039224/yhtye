//! The latest version of an adapter ([`super::AdapterChannel::Latest`]): asked
//! of npm with `npm view <package> version`, remembered so the network is not
//! used at every detection, and left unknown when npm cannot answer (the
//! adapter then runs the verified version, [`super::adapter_status`]).

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;

use super::adapter::{AdapterKind, parse_semver};
use super::detect::find_command;
use crate::console::NoConsoleWindow;

/// How long an answer of npm is kept.
pub const LATEST_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// How long a failure to ask is kept (the next detection does not wait for
/// npm again right away when the network is down).
pub const LATEST_FAILURE_TTL: Duration = Duration::from_secs(10 * 60);

/// How long `npm view` may take.
const NPM_VIEW_TIMEOUT: Duration = Duration::from_secs(10);

/// The npm executable next to `npx`.
const NPM_COMMAND: &str = "npm";

/// The file of [`NPM_COMMAND`] next to `npx`: npm installs a `.cmd` shim on
/// Windows, where `npx.cmd` is what the search finds.
const NPM_SIBLING: &str = if cfg!(windows) { "npm.cmd" } else { "npm" };

/// Asks for the newest version of an npm package.
#[async_trait]
pub trait VersionLookup: Send + Sync {
    /// The newest version of `package`, `None` when it could not be found out.
    /// `near` is the `npx` that will run the package, if the user named one
    /// (the `npm` next to it is the one to ask).
    async fn latest(&self, package: &str, near: Option<&Path>) -> Option<String>;
}

type EnvLookup = Arc<dyn Fn(&str) -> Option<OsString> + Send + Sync>;

/// [`VersionLookup`] through the `npm` command.
pub struct NpmView {
    env: EnvLookup,
    known_dirs: Vec<PathBuf>,
}

impl NpmView {
    /// Runs the `npm` found in the `PATH` of `env` (then in `known_dirs`).
    pub fn new(
        env: impl Fn(&str) -> Option<OsString> + Send + Sync + 'static,
        known_dirs: Vec<PathBuf>,
    ) -> Self {
        Self {
            env: Arc::new(env),
            known_dirs,
        }
    }

    fn npm(&self, near: Option<&Path>) -> Option<PathBuf> {
        let sibling = near
            .and_then(Path::parent)
            .map(|dir| dir.join(NPM_SIBLING))
            .filter(|p| p.is_file());
        sibling.or_else(|| {
            find_command(NPM_COMMAND, &*self.env, &self.known_dirs).map(|(path, _)| path)
        })
    }
}

#[async_trait]
impl VersionLookup for NpmView {
    async fn latest(&self, package: &str, near: Option<&Path>) -> Option<String> {
        let npm = self.npm(near)?;
        let run = tokio::process::Command::new(npm)
            .args(["view", package, "version"])
            .env("NPM_CONFIG_UPDATE_NOTIFIER", "false")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .no_console_window()
            .output();
        let output = match tokio::time::timeout(NPM_VIEW_TIMEOUT, run).await {
            Ok(Ok(output)) if output.status.success() => output,
            Ok(Ok(output)) => {
                tracing::warn!("npm view {package} version failed: {}", output.status);
                return None;
            }
            Ok(Err(e)) => {
                tracing::warn!("npm view {package} version could not run: {e}");
                return None;
            }
            Err(_) => {
                tracing::warn!("npm view {package} version timed out");
                return None;
            }
        };
        version_from_npm_view(&String::from_utf8_lossy(&output.stdout))
    }
}

/// The version `npm view <package> version` printed: its last non-empty line,
/// when that is a version.
#[must_use]
pub fn version_from_npm_view(stdout: &str) -> Option<String> {
    let line = stdout.lines().rev().find(|l| !l.trim().is_empty())?;
    parse_semver(line).ok()
}

#[derive(Debug, Clone)]
struct Entry {
    at: Instant,
    /// The answer; kept across a later failure.
    version: Option<String>,
    /// The last attempt succeeded.
    ok: bool,
}

/// The latest versions npm answered, by package.
pub struct LatestVersions {
    lookup: Arc<dyn VersionLookup>,
    entries: Mutex<HashMap<&'static str, Entry>>,
}

impl LatestVersions {
    #[must_use]
    pub fn new(lookup: Arc<dyn VersionLookup>) -> Self {
        Self {
            lookup,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// The newest version known of `kind`, however old the answer is.
    #[must_use]
    pub fn get(&self, kind: AdapterKind) -> Option<String> {
        self.lock().get(kind.package())?.version.clone()
    }

    /// The known versions by package (what [`super::detect_harnesses_with`] reads).
    #[must_use]
    pub fn known(&self) -> HashMap<String, String> {
        self.lock()
            .iter()
            .filter_map(|(package, e)| Some(((*package).to_string(), e.version.clone()?)))
            .collect()
    }

    /// Asks npm for the adapters in `kinds` whose answer is missing or older
    /// than [`LATEST_TTL`] ([`LATEST_FAILURE_TTL`] after a failure); `force`
    /// asks for all of them. A failure keeps the version known before, if any.
    pub async fn refresh(&self, kinds: &[AdapterKind], near: Option<&Path>, force: bool) {
        self.refresh_at(kinds, near, force, Instant::now()).await;
    }

    async fn refresh_at(
        &self,
        kinds: &[AdapterKind],
        near: Option<&Path>,
        force: bool,
        now: Instant,
    ) {
        let stale: Vec<AdapterKind> = kinds
            .iter()
            .copied()
            .filter(|kind| force || self.is_stale(*kind, now))
            .collect();
        let answers = futures::future::join_all(
            stale
                .iter()
                .map(|kind| self.lookup.latest(kind.package(), near)),
        )
        .await;
        let mut entries = self.lock();
        for (kind, answer) in stale.into_iter().zip(answers) {
            let previous = entries.get(kind.package()).and_then(|e| e.version.clone());
            let ok = answer.is_some();
            entries.insert(
                kind.package(),
                Entry {
                    at: now,
                    version: answer.or(previous),
                    ok,
                },
            );
        }
    }

    fn is_stale(&self, kind: AdapterKind, now: Instant) -> bool {
        match self.lock().get(kind.package()) {
            None => true,
            Some(e) => {
                let ttl = if e.ok { LATEST_TTL } else { LATEST_FAILURE_TTL };
                now.saturating_duration_since(e.at) >= ttl
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<&'static str, Entry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl std::fmt::Debug for LatestVersions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LatestVersions")
            .field("known", &self.known())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Answers from a script (one answer per call, the last one repeats) and
    /// counts the calls: no network.
    struct Scripted {
        answers: Mutex<Vec<Option<String>>>,
        calls: AtomicUsize,
    }

    impl Scripted {
        fn new(answers: &[Option<&str>]) -> Arc<Self> {
            Arc::new(Self {
                answers: Mutex::new(
                    answers
                        .iter()
                        .map(|a| a.map(str::to_string))
                        .rev()
                        .collect(),
                ),
                calls: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl VersionLookup for Scripted {
        async fn latest(&self, _package: &str, _near: Option<&Path>) -> Option<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut answers = self.answers.lock().expect("lock");
            if answers.len() > 1 {
                answers.pop().flatten()
            } else {
                answers.last().cloned().flatten()
            }
        }
    }

    #[test]
    fn npm_view_output_is_read_as_a_version_or_nothing() {
        assert_eq!(version_from_npm_view("0.109.0\n"), Some("0.109.0".into()));
        assert_eq!(
            version_from_npm_view("npm notice\n\n2.8.0\n\n"),
            Some("2.8.0".into()),
            "the last line is the answer"
        );
        assert_eq!(version_from_npm_view(""), None);
        assert_eq!(version_from_npm_view("\n \n"), None);
        assert_eq!(version_from_npm_view("not a version"), None);
        assert_eq!(version_from_npm_view("1.2.3\nlatest"), None);
        assert_eq!(version_from_npm_view("1.2.3 --yes"), None);
        assert_eq!(version_from_npm_view("{\"version\":\"1.2.3\"}"), None);
    }

    #[tokio::test]
    async fn an_answer_is_remembered_until_it_is_old() {
        let lookup = Scripted::new(&[Some("0.109.0"), Some("0.110.0")]);
        let latest = LatestVersions::new(lookup.clone());
        let kinds = [AdapterKind::ClaudeAgent];
        let start = Instant::now();
        assert_eq!(latest.get(AdapterKind::ClaudeAgent), None, "nothing yet");

        latest.refresh_at(&kinds, None, false, start).await;
        assert_eq!(latest.get(AdapterKind::ClaudeAgent), Some("0.109.0".into()));
        assert_eq!(lookup.calls(), 1);

        latest
            .refresh_at(&kinds, None, false, start + Duration::from_secs(60))
            .await;
        assert_eq!(lookup.calls(), 1, "the network is not used again at once");

        latest
            .refresh_at(&kinds, None, false, start + LATEST_TTL)
            .await;
        assert_eq!(lookup.calls(), 2);
        assert_eq!(latest.get(AdapterKind::ClaudeAgent), Some("0.110.0".into()));
        assert_eq!(
            latest.known(),
            HashMap::from([(
                "@agentclientprotocol/claude-agent-acp".to_string(),
                "0.110.0".to_string()
            )])
        );
    }

    #[tokio::test]
    async fn forcing_asks_again_and_only_the_adapters_that_are_wanted() {
        let lookup = Scripted::new(&[Some("1.0.0")]);
        let latest = LatestVersions::new(lookup.clone());
        let start = Instant::now();
        latest
            .refresh_at(&[AdapterKind::Codex], None, false, start)
            .await;
        assert_eq!(lookup.calls(), 1);
        assert_eq!(latest.get(AdapterKind::ClaudeAgent), None, "not asked for");
        latest
            .refresh_at(&[AdapterKind::Codex], None, true, start)
            .await;
        assert_eq!(lookup.calls(), 2, "forced");
        latest
            .refresh_at(&AdapterKind::ALL, None, false, start)
            .await;
        assert_eq!(lookup.calls(), 3, "only the one that was never asked");
    }

    #[tokio::test]
    async fn a_failure_is_not_retried_for_a_while_and_keeps_the_earlier_answer() {
        let lookup = Scripted::new(&[None, Some("2.8.0"), None]);
        let latest = LatestVersions::new(lookup.clone());
        let kinds = [AdapterKind::Codex];
        let start = Instant::now();

        latest.refresh_at(&kinds, None, false, start).await;
        assert_eq!(
            latest.get(AdapterKind::Codex),
            None,
            "unknown: verified runs"
        );
        assert_eq!(lookup.calls(), 1);
        latest
            .refresh_at(&kinds, None, false, start + Duration::from_secs(30))
            .await;
        assert_eq!(lookup.calls(), 1, "no waiting for a dead network again");

        latest
            .refresh_at(&kinds, None, false, start + LATEST_FAILURE_TTL)
            .await;
        assert_eq!(latest.get(AdapterKind::Codex), Some("2.8.0".into()));
        assert_eq!(lookup.calls(), 2);

        // Later npm cannot be asked: the version found before stays.
        latest
            .refresh_at(&kinds, None, false, start + LATEST_FAILURE_TTL + LATEST_TTL)
            .await;
        assert_eq!(lookup.calls(), 3);
        assert_eq!(latest.get(AdapterKind::Codex), Some("2.8.0".into()));
    }
}
