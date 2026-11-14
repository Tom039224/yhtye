//! Windows side of the agent process tree.
//!
//! Provisional: only the launcher itself is stopped (stdin EOF, then
//! `Child::kill`, plus `kill_on_drop`). Descendants of a wrapper launcher and a
//! Yhtye that dies without cleaning up are not covered yet; a Job Object is
//! meant to take over here, which is why everything platform-specific is behind
//! this small surface (`configure_command`, `GroupKillGuard`, `exit_parts`).

use std::process::ExitStatus;

use tokio::process::Command;

/// `CREATE_NO_WINDOW`: a console-subsystem agent started from the GUI must not
/// open a console window of its own.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Starts the agent without a console window.
pub(crate) fn configure_command(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// Stands for the agent's whole process tree. Nothing to guard yet.
pub(crate) struct GroupKillGuard;

impl GroupKillGuard {
    pub(crate) fn new(_pid: Option<u32>, _command: &str) -> Self {
        Self
    }

    /// No group to ask: the launcher is stopped through its own handle.
    pub(crate) fn request_stop(&self) {}

    /// No group to kill: the launcher is killed through its own handle.
    pub(crate) fn kill(&self) {}

    pub(crate) fn disarm(&mut self) {}
}

/// `(exit code, terminating signal)` of a status; Windows has no signals.
pub(crate) fn exit_parts(status: Option<ExitStatus>) -> (Option<i32>, Option<i32>) {
    (status.and_then(|s| s.code()), None)
}
