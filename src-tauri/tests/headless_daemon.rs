//! `yhtye --headless -d` against the real binary: the launcher ends, the daemon
//! goes on in a session of its own with its output in the log, answers
//! `remote status`, refuses a second one on the same data directory, and stops
//! on `remote stop`. No display is needed (and none is used).

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_yhtye");
const EXIT_LOCK_TAKEN: i32 = 3;

/// `yhtye` with an environment that finds no Tailscale (the real one must not
/// be bound) and no session bus (no keyring), and no display.
fn yhtye(path_dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.args(args)
        .env("PATH", path_dir)
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null());
    cmd
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn describe(output: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        text(&output.stdout),
        text(&output.stderr)
    )
}

/// Stops the daemon when the test ends, also when it fails.
struct Daemon {
    pid: Option<u32>,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            let _ = Command::new("kill")
                .arg("-KILL")
                .arg(pid.to_string())
                .status();
        }
    }
}

/// The fields of `/proc/<pid>/stat` after the command name: state, ppid, pgrp, session, …
fn stat_fields(pid: u32) -> Vec<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("stat");
    let after_name = stat.rsplit_once(')').expect("a command name").1;
    after_name.split_whitespace().map(str::to_string).collect()
}

fn try_pid_of(text: &str) -> Option<u32> {
    text.split("pid ")
        .nth(1)
        .and_then(|rest| {
            rest.trim_start()
                .split(|c: char| !c.is_ascii_digit())
                .next()
        })
        .and_then(|digits| digits.parse().ok())
}

fn pid_of(text: &str) -> u32 {
    try_pid_of(text).unwrap_or_else(|| panic!("a pid in {text}"))
}

fn free_port() -> String {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port()
        .to_string()
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_daemon_outlives_its_launcher_and_obeys_remote_stop() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let empty_path = tmp.path().join("empty-path");
    std::fs::create_dir(&empty_path).expect("path dir");
    let data = tmp.path().join("data");
    let data_arg = data.to_str().expect("utf-8 path").to_string();
    let port = free_port();
    let mut daemon = Daemon { pid: None };

    // Launch: the launcher prints where the daemon is and ends with success.
    let launch = yhtye(
        &empty_path,
        &["--headless", "-d", "--data-dir", &data_arg, "--port", &port],
    )
    .output()
    .expect("launcher");
    let out = text(&launch.stdout);
    // Before anything can fail: the guard stops what was started.
    daemon.pid = try_pid_of(&out);
    assert!(launch.status.success(), "{}", describe(&launch));
    let log: PathBuf = data.join("headless.log");
    let pid = daemon.pid.expect("a pid in the launcher's report");
    assert!(out.contains(&format!("log   {}", log.display())), "{out}");
    assert!(out.contains("yhtye remote pair"), "{out}");
    assert!(out.contains("yhtye remote stop"), "{out}");
    assert!(
        !out.to_lowercase().contains("code  "),
        "no pairing code in a daemon launch: {out}"
    );

    // The daemon: alive after the launcher's end, in a session of its own, with
    // /dev/null for input and the log for output.
    let fields = stat_fields(pid);
    assert_eq!(fields[3], pid.to_string(), "it leads its own session");
    assert_ne!(
        fields[1],
        std::process::id().to_string(),
        "not our child any more"
    );
    let link = |fd: u32| std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).expect("fd");
    assert_eq!(link(0), Path::new("/dev/null"));
    assert_eq!(link(1), log);
    assert_eq!(link(2), log);

    // `remote status` answers, with the pid; Tailscale is missing, and it says so.
    let status = yhtye(&empty_path, &["remote", "status", "--data-dir", &data_arg])
        .output()
        .expect("status");
    assert!(status.status.success(), "{}", describe(&status));
    let status_text = text(&status.stdout);
    assert_eq!(pid_of(&status_text), pid, "{status_text}");
    assert!(status_text.contains("running (headless"), "{status_text}");

    // A second daemon on the same data directory fails with the lock's exit code.
    let second = yhtye(&empty_path, &["--headless", "-d", "--data-dir", &data_arg])
        .output()
        .expect("second launcher");
    assert_eq!(
        second.status.code(),
        Some(EXIT_LOCK_TAKEN),
        "{}",
        describe(&second)
    );
    assert!(
        text(&second.stderr).contains("already running"),
        "{}",
        describe(&second)
    );
    assert!(stat_fields(pid)[0] != "Z", "the first one is untouched");

    // The log has what the daemon said (stdout and stderr both).
    let log_text = std::fs::read_to_string(&log).expect("log");
    assert!(
        log_text.contains("Yhtye (headless) is running on"),
        "{log_text}"
    );
    assert!(log_text.contains("headless: data dir"), "{log_text}");

    // Stop: waits until the daemon has ended, and its socket is gone.
    let stop = yhtye(&empty_path, &["remote", "stop", "--data-dir", &data_arg])
        .output()
        .expect("stop");
    assert!(stop.status.success(), "{}", describe(&stop));
    assert!(
        text(&stop.stdout).contains("Stopped."),
        "{}",
        describe(&stop)
    );
    wait_until("the process is gone", || {
        !Path::new(&format!("/proc/{pid}")).exists()
    });
    assert!(!data.join("yhtye.sock").exists(), "the socket is removed");
    let after = yhtye(&empty_path, &["remote", "status", "--data-dir", &data_arg])
        .output()
        .expect("status");
    assert!(!after.status.success());
    assert!(
        text(&after.stderr).contains("no headless Yhtye answers"),
        "{}",
        describe(&after)
    );
    let log_text = std::fs::read_to_string(&log).expect("log");
    assert!(log_text.contains("all agents stopped"), "{log_text}");
    daemon.pid = None;
}

#[test]
fn a_daemon_needs_headless_and_the_app_without_a_display_says_so() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let data_arg = data.to_str().expect("utf-8 path").to_string();

    let no_headless = yhtye(tmp.path(), &["-d", "--data-dir", &data_arg])
        .output()
        .expect("run");
    assert_eq!(
        no_headless.status.code(),
        Some(2),
        "{}",
        describe(&no_headless)
    );
    assert!(text(&no_headless.stderr).contains("--headless"));

    let no_display = yhtye(tmp.path(), &["--data-dir", &data_arg])
        .output()
        .expect("run");
    assert_eq!(
        no_display.status.code(),
        Some(1),
        "{}",
        describe(&no_display)
    );
    assert!(text(&no_display.stderr).contains("--headless"));
    assert!(!data.exists(), "nothing was started");
}
