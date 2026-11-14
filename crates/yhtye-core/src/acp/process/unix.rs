//! Unix side of the agent process tree: the agent leads its own process group, which
//! is signalled as a whole (EOF → SIGTERM → SIGKILL) so wrapper launchers (`npx` →
//! `node`) leave no orphans. If Yhtye itself dies first, `parent_death` ends the
//! group and `orphan` sweeps what is left at the next start.

use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use rustix::process::{Pid, Signal};
use tokio::process::{Child, Command};

use crate::acp::orphan::{self, Registration};
use crate::acp::parent_death::{self, Watchdog};

/// Makes the agent the leader of a new process group and arms the kernel to end
/// it when Yhtye dies.
pub(crate) fn configure_command(cmd: &mut Command) {
    cmd.process_group(0);
    parent_death::arm_child(cmd);
}

/// Kills the agent's process group (SIGKILL) when dropped, unless it already exited
/// and was cleaned up via [`super::terminate`]. While it lives, the group is also
/// recorded on disk and watched, so a Yhtye that dies without dropping it leaves no
/// orphan (`parent_death`, `orphan`).
pub(crate) struct GroupKillGuard {
    pgid: Option<Pid>,
    /// The on-disk record the next start sweeps by; removed with the guard.
    registration: Option<Registration>,
    /// Kills the group if Yhtye dies; stood down with the guard.
    watchdog: Option<Watchdog>,
}

impl GroupKillGuard {
    /// Guards the group led by the freshly spawned agent `child` (nothing to guard
    /// when the process is already gone). Never fails; the `Result` is the shape
    /// the Windows side needs.
    pub(crate) fn new(child: &Child, command: &str) -> io::Result<Self> {
        let pgid = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(Pid::from_raw);
        Ok(Self {
            pgid,
            registration: pgid.and_then(|pgid| orphan::register(pgid, command)),
            watchdog: pgid.and_then(Watchdog::start),
        })
    }

    fn signal(&self, signal: Signal) {
        if let Some(pgid) = self.pgid {
            // ESRCH just means the group is already gone.
            let _ = rustix::process::kill_process_group(pgid, signal);
        }
    }

    /// Asks the whole group to stop (SIGTERM).
    pub(crate) fn request_stop(&self) {
        self.signal(Signal::TERM);
    }

    /// Kills the whole group (SIGKILL).
    pub(crate) fn kill(&self) {
        self.signal(Signal::KILL);
    }

    /// Kills whatever is left of the group (a launcher may exit while descendants
    /// keep running) and stops tracking it.
    pub(crate) fn disarm(&mut self) {
        self.kill();
        self.pgid = None;
        self.registration = None;
        self.watchdog = None;
    }
}

impl Drop for GroupKillGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

/// `(exit code, terminating signal)` of a status.
pub(crate) fn exit_parts(status: Option<ExitStatus>) -> (Option<i32>, Option<i32>) {
    status.map_or((None, None), |s| (s.code(), s.signal()))
}
