//! Agent subprocess management: launch in its own process group, forward stderr,
//! and terminate the whole group so wrapper launchers (`npx` → `node`) leave no orphans.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc::UnboundedSender;

use super::config::HarnessConfig;
use super::events::{AgentError, AgentEvent};

/// Number of stderr lines kept for error messages.
const STDERR_TAIL_LINES: usize = 40;
/// Time an agent gets to exit by itself after its stdin closes.
const EXIT_AFTER_EOF_GRACE: Duration = Duration::from_secs(1);
/// Time an agent gets to exit after SIGTERM before SIGKILL.
const EXIT_AFTER_TERM_GRACE: Duration = Duration::from_secs(2);

/// A launched agent process and its protocol pipes.
pub(crate) struct AgentProcess {
    pub child: Child,
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
    pub group: GroupKillGuard,
    pub stderr_tail: StderrTail,
    /// Finishes when the agent's stderr reaches EOF.
    pub stderr_task: tokio::task::JoinHandle<()>,
}

/// Last stderr lines of the agent, shared with the stderr reader task.
#[derive(Clone, Default)]
pub(crate) struct StderrTail(Arc<Mutex<VecDeque<String>>>);

impl StderrTail {
    fn push(&self, line: String) {
        let Ok(mut lines) = self.0.lock() else { return };
        if lines.len() == STDERR_TAIL_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    /// The captured tail joined with newlines (empty when nothing was written).
    pub fn text(&self) -> String {
        self.0
            .lock()
            .map(|l| l.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }

    /// `message` plus the stderr tail, for startup error reports.
    pub fn annotate(&self, message: impl Into<String>) -> String {
        let message = message.into();
        let tail = self.text();
        if tail.is_empty() {
            message
        } else {
            format!("{message}\n--- agent stderr (tail) ---\n{tail}")
        }
    }
}

/// Kills the agent's process group (SIGKILL) when dropped, unless it already exited
/// and was cleaned up via [`terminate`].
pub(crate) struct GroupKillGuard {
    pgid: Option<rustix::process::Pid>,
}

impl GroupKillGuard {
    fn signal(&self, signal: rustix::process::Signal) {
        if let Some(pgid) = self.pgid {
            // ESRCH just means the group is already gone.
            let _ = rustix::process::kill_process_group(pgid, signal);
        }
    }
}

impl GroupKillGuard {
    /// Kills whatever is left of the group (a launcher may exit while descendants
    /// keep running) and stops tracking it.
    fn disarm(&mut self) {
        self.signal(rustix::process::Signal::KILL);
        self.pgid = None;
    }
}

impl Drop for GroupKillGuard {
    fn drop(&mut self) {
        self.signal(rustix::process::Signal::KILL);
    }
}

/// Environment variables of Yhtye itself that agents must not inherit (the dev
/// bridge's token would let an agent drive the core directly, bypassing MCP).
const PRIVATE_ENV: &[&str] = &["YHTYE_BRIDGE_TOKEN", "VITE_YHTYE_BRIDGE_TOKEN"];

/// Directory the agent *process* starts in: the user's home (or `/`), never the
/// project. The session's working directory (`cwd`) reaches the agent through
/// ACP (`session/new` / `session/load`). Starting in the project would let an
/// untrusted repository steer the launcher before any agent runs — e.g. `npx`
/// reads the project's `.npmrc` (registry) and prefers its `node_modules`
/// (`core-design.md` §3.4, Stage 6b security review).
fn launch_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir())
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Launches the harness command as the leader of a new process group, for a
/// session that will work in `cwd` (see [`launch_dir`]).
pub(crate) fn spawn_process(
    harness: &HarnessConfig,
    cwd: &Path,
    events: UnboundedSender<AgentEvent>,
) -> Result<AgentProcess, AgentError> {
    let launch = launch_dir();
    let mut cmd = Command::new(&harness.command);
    for name in PRIVATE_ENV
        .iter()
        .copied()
        .chain(harness.env_remove.iter().map(String::as_str))
    {
        cmd.env_remove(name);
    }
    cmd.args(&harness.args)
        .envs(&harness.env)
        .current_dir(&launch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    let spawn_error = |message: String| AgentError::Spawn {
        command: harness.command.clone(),
        message,
    };
    let mut child = cmd.spawn().map_err(|e| {
        spawn_error(format!(
            "{e} (started in {} for a session in {})",
            launch.display(),
            cwd.display()
        ))
    })?;
    let group = GroupKillGuard {
        pgid: child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(rustix::process::Pid::from_raw),
    };
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err(spawn_error("stdio pipes were not created".into()));
    };
    let stderr_tail = StderrTail::default();
    let stderr_task = tokio::spawn(forward_stderr(stderr, stderr_tail.clone(), events));
    Ok(AgentProcess {
        child,
        stdin,
        stdout,
        group,
        stderr_tail,
        stderr_task,
    })
}

async fn forward_stderr(
    stderr: ChildStderr,
    tail: StderrTail,
    events: UnboundedSender<AgentEvent>,
) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::debug!(target: "yhtye_core::acp::stderr", "{line}");
        tail.push(line.clone());
        let _ = events.send(AgentEvent::Stderr(line));
    }
}

/// Waits for the agent to exit (escalating EOF → SIGTERM → SIGKILL on the whole group),
/// then kills any remaining group members. Returns the exit status if it could be read.
pub(crate) async fn terminate(child: &mut Child, group: &mut GroupKillGuard) -> Option<ExitStatus> {
    if let Ok(Ok(status)) = tokio::time::timeout(EXIT_AFTER_EOF_GRACE, child.wait()).await {
        group.disarm();
        return Some(status);
    }
    group.signal(rustix::process::Signal::TERM);
    let status = match tokio::time::timeout(EXIT_AFTER_TERM_GRACE, child.wait()).await {
        Ok(Ok(status)) => Some(status),
        _ => {
            group.signal(rustix::process::Signal::KILL);
            child.wait().await.ok()
        }
    };
    group.disarm();
    status
}

/// `(exit code, terminating signal)` of a status.
pub(crate) fn exit_parts(status: Option<ExitStatus>) -> (Option<i32>, Option<i32>) {
    use std::os::unix::process::ExitStatusExt;
    status.map_or((None, None), |s| (s.code(), s.signal()))
}
