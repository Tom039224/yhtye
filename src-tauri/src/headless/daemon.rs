//! `yhtye --headless -d` (`--daemon`): runs the headless instance detached from
//! the terminal, so that it goes on when the terminal is closed, without a
//! service manager.
//!
//! The launching process ([`launch`]) starts a copy of itself with the internal
//! flag `--daemon-child` and returns once that copy answers on its control
//! socket. The copy ([`detach_from_terminal`]) leaves the terminal's session
//! (`setsid`: no controlling terminal, so no SIGHUP when it closes) and ignores
//! SIGHUP; its stdin is `/dev/null` and its stdout and stderr are the log file
//! [`LOG_FILE`] in the data directory. The copy takes the data directory's lock
//! (`yhtye.lock`), not the launcher: when another Yhtye holds it the copy exits
//! with [`super::EXIT_LOCK_TAKEN`] and the launcher reports that.

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use yhtye_core::api::ApiCommand;

use super::control::{self, Client};
use super::{say, EXIT_LOCK_TAKEN};
use crate::cli::Options;
use crate::data_dir::DataDir;
use crate::data_lock::LockError;

/// Name of the log file in the data directory (appended to on every start).
pub const LOG_FILE: &str = "headless.log";

/// How long the launcher waits for the daemon to answer before it gives up
/// (and kills it).
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

/// How often the launcher looks.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How much of the log's new part is shown when the daemon fails to start.
const LOG_TAIL_BYTES: u64 = 4096;
const LOG_TAIL_LINES: usize = 20;

/// Why the daemon did not start: the message, and the exit code to end with.
#[derive(Debug)]
struct LaunchError {
    message: String,
    code: u8,
}

impl LaunchError {
    fn failure(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: 1,
        }
    }
}

/// Starts the daemon and reports where it is; the process ends then (the
/// daemon goes on).
pub fn launch(opts: &Options) -> ExitCode {
    match try_launch(opts) {
        Ok(started) => {
            say(&started.report());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Yhtye: {}", e.message);
            ExitCode::from(e.code)
        }
    }
}

/// A daemon that answers.
struct Started {
    pid: u32,
    log: PathBuf,
    dir: DataDir,
}

impl Started {
    /// What the launcher prints before it ends.
    fn report(&self) -> String {
        let dir = &self.dir.path;
        // A named directory has to be named again to find the daemon.
        let named = if self.dir.is_default {
            String::new()
        } else {
            format!(" --data-dir {}", dir.display())
        };
        format!(
            "Yhtye (headless) runs in the background.\n\
             \x20 pid   {pid}\n\
             \x20 log   {log}\n\
             Pair an Android device:  yhtye remote pair{named}\n\
             Stop it:                 yhtye remote stop{named}   (or: kill {pid})",
            pid = self.pid,
            log = self.log.display(),
        )
    }
}

fn try_launch(opts: &Options) -> Result<Started, LaunchError> {
    let dir = DataDir::resolve(opts.data_dir.as_deref())
        .and_then(|d| d.prepare().map(|()| d))
        .map_err(|e| LaunchError::failure(e.to_string()))?;
    let log_path = dir.path.join(LOG_FILE);
    let log = open_log(&log_path)
        .map_err(|e| LaunchError::failure(format!("cannot open {}: {e}", log_path.display())))?;
    // Where this start's part of the log begins.
    let log_start = log.metadata().map_or(0, |m| m.len());
    let exe = std::env::current_exe()
        .map_err(|e| LaunchError::failure(format!("cannot tell which program this is: {e}")))?;
    let stderr = log
        .try_clone()
        .map_err(|e| LaunchError::failure(format!("cannot use {}: {e}", log_path.display())))?;
    let mut child = Command::new(exe)
        .args(child_args(opts, &dir.path))
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(stderr)
        .spawn()
        .map_err(|e| LaunchError::failure(format!("cannot start the daemon: {e}")))?;
    let ended = Ended {
        dir: &dir.path,
        log: &log_path,
        log_start,
    };
    wait_until_ready(&mut child, &control::socket_path(&dir.path), &ended)?;
    Ok(Started {
        pid: child.id(),
        log: log_path,
        dir,
    })
}

/// The arguments of the detached copy: the same options, with the data
/// directory spelled out (the launcher has resolved it and moved a legacy one).
fn child_args(opts: &Options, data_dir: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "--headless".into(),
        "--daemon-child".into(),
        "--data-dir".into(),
        data_dir.into(),
    ];
    if let Some(port) = opts.port {
        args.extend(["--port".into(), port.to_string().into()]);
    }
    if let Some(model) = &opts.model {
        args.extend(["--model".into(), model.into()]);
    }
    args
}

/// The log, appended to; readable by this user only (it names projects and paths).
fn open_log(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

/// Where a start went wrong, for the message.
struct Ended<'a> {
    dir: &'a Path,
    log: &'a Path,
    log_start: u64,
}

impl Ended<'_> {
    /// The error for a daemon that exited during start.
    fn exited(&self, status: ExitStatus) -> LaunchError {
        if status.code() == Some(i32::from(EXIT_LOCK_TAKEN)) {
            return LaunchError {
                message: format!(
                    "{}\n(a headless one is shown by `yhtye remote status` and stopped by `yhtye remote stop`)",
                    LockError::Taken(self.dir.to_path_buf())
                ),
                code: EXIT_LOCK_TAKEN,
            };
        }
        let how = match (status.code(), status.signal()) {
            (Some(code), _) => format!("exit status {code}"),
            (None, Some(signal)) => format!("signal {signal}"),
            (None, None) => "an unknown status".to_string(),
        };
        LaunchError::failure(format!(
            "the daemon stopped while starting ({how}); see {}{}",
            self.log.display(),
            log_tail(self.log, self.log_start)
                .map(|tail| format!(":\n{tail}"))
                .unwrap_or_default()
        ))
    }

    fn timed_out(&self) -> LaunchError {
        LaunchError::failure(format!(
            "the daemon did not answer within {} s and was stopped; see {}",
            STARTUP_TIMEOUT.as_secs(),
            self.log.display()
        ))
    }
}

/// Waits until the daemon answers on its control socket (as the process
/// `child`: a socket of another instance does not count), or has exited.
fn wait_until_ready(child: &mut Child, socket: &Path, ended: &Ended) -> Result<(), LaunchError> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Err(ended.exited(status)),
            Ok(None) => {}
            Err(e) => {
                return Err(LaunchError::failure(format!(
                    "cannot watch the daemon: {e}"
                )))
            }
        }
        if answers(socket, child.id()) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ended.timed_out());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// The process `pid` answers on `socket`.
fn answers(socket: &Path, pid: u32) -> bool {
    let Ok(mut client) = Client::connect(socket) else {
        return false;
    };
    // Where the system can name the listener, it must be the daemon we started.
    if client.server_pid().is_some_and(|server| server != pid) {
        return false;
    }
    client.call(&ApiCommand::GetRemoteStatus).is_ok()
}

/// The last lines of what was appended to the log since `from`, if any.
fn log_tail(path: &Path, from: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(LOG_TAIL_BYTES).max(from);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut text = String::new();
    file.take(LOG_TAIL_BYTES).read_to_string(&mut text).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines[lines.len().saturating_sub(LOG_TAIL_LINES)..].join("\n");
    (!tail.trim().is_empty()).then_some(tail)
}

/// Called by the detached copy first thing: leaves the terminal's session, so
/// that closing the terminal does not reach it.
pub fn detach_from_terminal() {
    if let Err(e) = rustix::process::setsid() {
        // Already a session leader (started by hand): nothing to leave.
        tracing::warn!("could not start a new session: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Daemon;

    #[test]
    fn the_copy_gets_the_resolved_data_dir_and_the_options_it_must_keep() {
        let mut opts = Options {
            daemon: Daemon::Launch,
            ..Options::default()
        };
        let args = child_args(&opts, Path::new("/srv/yhtye"));
        assert_eq!(
            args,
            ["--headless", "--daemon-child", "--data-dir", "/srv/yhtye"]
        );
        opts.port = Some(1500);
        opts.model = Some("sonnet".into());
        let args = child_args(&opts, Path::new("/srv/yhtye"));
        assert_eq!(
            args,
            [
                "--headless",
                "--daemon-child",
                "--data-dir",
                "/srv/yhtye",
                "--port",
                "1500",
                "--model",
                "sonnet"
            ]
        );
    }

    #[test]
    fn the_copy_args_parse_back_into_a_child_headless_instance() {
        use crate::cli::{parse, Invocation};
        let opts = Options {
            port: Some(1500),
            model: Some("opus".into()),
            ..Options::default()
        };
        let Ok(Invocation::Headless(child)) = parse(child_args(&opts, Path::new("/d"))) else {
            panic!("a headless invocation");
        };
        assert_eq!(child.daemon, Daemon::Child);
        assert_eq!(child.data_dir.as_deref(), Some(Path::new("/d")));
        assert_eq!(child.port, Some(1500));
        assert_eq!(child.model.as_deref(), Some("opus"));
    }

    #[test]
    fn the_log_is_appended_to_and_private() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join(LOG_FILE);
        writeln!(open_log(&path).expect("log"), "first").expect("write");
        writeln!(open_log(&path).expect("log"), "second").expect("write");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "first\nsecond\n"
        );
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_failed_start_shows_only_the_new_end_of_the_log() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join(LOG_FILE);
        let old = "an earlier run\n";
        let mut text = String::from(old);
        for i in 0..30 {
            text.push_str(&format!("line {i}\n"));
        }
        std::fs::write(&path, &text).expect("write");
        let tail = log_tail(&path, old.len() as u64).expect("a tail");
        let lines: Vec<&str> = tail.lines().collect();
        assert_eq!(lines.len(), LOG_TAIL_LINES);
        assert_eq!(lines.last(), Some(&"line 29"));
        assert!(!tail.contains("earlier"));
        // Nothing new: nothing to show.
        assert_eq!(log_tail(&path, text.len() as u64), None);
    }

    #[test]
    fn an_exit_for_a_taken_data_directory_is_reported_with_its_own_code() {
        let ended = Ended {
            dir: Path::new("/srv/yhtye"),
            log: Path::new("/srv/yhtye/headless.log"),
            log_start: 0,
        };
        let taken = ended.exited(ExitStatus::from_raw(i32::from(EXIT_LOCK_TAKEN) << 8));
        assert_eq!(taken.code, EXIT_LOCK_TAKEN);
        assert!(
            taken.message.contains("already running on /srv/yhtye"),
            "{}",
            taken.message
        );
        let other = ended.exited(ExitStatus::from_raw(1 << 8));
        assert_eq!(other.code, 1);
        assert!(other.message.contains("exit status 1"), "{}", other.message);
        assert!(other.message.contains("headless.log"));
        // Killed by SIGKILL (the raw status is the signal number).
        let killed = ended.exited(ExitStatus::from_raw(9));
        assert!(killed.message.contains("signal 9"), "{}", killed.message);
    }

    #[test]
    fn the_report_says_where_the_daemon_is_and_how_to_use_and_stop_it() {
        let started = Started {
            pid: 4242,
            log: PathBuf::from("/home/u/.local/share/io.github.tom039224.yhtye/headless.log"),
            dir: DataDir {
                path: PathBuf::from("/home/u/.local/share/io.github.tom039224.yhtye"),
                is_default: true,
            },
        };
        let text = started.report();
        assert!(text.contains("pid   4242"), "{text}");
        assert!(text.contains("log   /home/u/.local/share/io.github.tom039224.yhtye/headless.log"));
        assert!(text.contains("yhtye remote pair\n"), "{text}");
        assert!(
            text.contains("yhtye remote stop   (or: kill 4242)"),
            "{text}"
        );
        assert!(!text.contains("--data-dir"));

        let named = Started {
            dir: DataDir {
                path: PathBuf::from("/srv/yhtye"),
                is_default: false,
            },
            ..started
        };
        let text = named.report();
        assert!(
            text.contains("yhtye remote pair --data-dir /srv/yhtye\n"),
            "{text}"
        );
        assert!(text.contains("yhtye remote stop --data-dir /srv/yhtye   (or: kill 4242)"));
    }
}
