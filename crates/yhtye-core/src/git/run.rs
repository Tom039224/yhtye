//! Running the `git` CLI: a clean environment, no prompts, a timeout.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

/// Longest a single git command may take (git runs inside the runtime loop).
pub(super) const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Environment variables that would point git at another repository or inject
/// configuration (e.g. when Yhtye itself runs inside a git hook or under
/// `git -c`). They are removed for every command.
const REPO_ENV: [&str; 9] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
];

/// Output of a finished git command.
#[derive(Debug, Clone)]
pub(super) struct Out {
    pub(super) ok: bool,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

impl Out {
    /// `stdout` if the command succeeded, otherwise a message naming it.
    pub(super) fn into_result(self, what: &str) -> Result<String, String> {
        if self.ok {
            Ok(self.stdout)
        } else {
            Err(self.failure(what))
        }
    }

    pub(super) fn failure(&self, what: &str) -> String {
        let detail = if self.stderr.trim().is_empty() {
            self.stdout.trim()
        } else {
            self.stderr.trim()
        };
        format!("{what} failed: {detail}")
    }

    /// Non-empty lines of stdout.
    pub(super) fn lines(&self) -> Vec<String> {
        self.stdout
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// Runs `git <args>` in `dir` with extra environment `env`. `Err` only when git
/// could not run or timed out; a failing command is an `Out` with `ok = false`.
pub(super) async fn git_env(
    dir: &Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<Out, String> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_MERGE_AUTOEDIT", "no");
    for var in REPO_ENV {
        cmd.env_remove(var);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let shown = args.join(" ");
    let output = tokio::time::timeout(GIT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("git {shown} timed out after {}s", GIT_TIMEOUT.as_secs()))?
        .map_err(|e| format!("could not run git {shown} in {}: {e}", dir.display()))?;
    Ok(Out {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Runs `git <args>` in `dir`.
pub(super) async fn git(dir: &Path, args: &[&str]) -> Result<Out, String> {
    git_env(dir, args, &[]).await
}

/// Runs `git <args>` in `dir` and returns stdout, failing if git fails.
pub(super) async fn git_ok(dir: &Path, args: &[&str]) -> Result<String, String> {
    git(dir, args)
        .await?
        .into_result(&format!("git {}", args.join(" ")))
}
