//! Agents must not outlive Yhtye, however it ends (SIGKILL, crash, abort): the
//! normal shutdown path (`process::terminate`) never runs then.
//!
//! Two layers, both Linux-only (other systems rely on the stdin EOF the agent
//! sees). `orphan` adds a third: the next start sweeps what these miss.
//!
//! - [`arm_child`]: the kernel kills the launcher (`npx`, the agent binary)
//!   when the thread that started it ends. That alone leaves its descendants —
//!   `npx` → `node` → `claude` — running in the orphaned process group.
//! - [`Watchdog`]: a small `sh` in its own process group blocks on a pipe whose
//!   write end Yhtye holds. When Yhtye dies the kernel closes the pipe and the
//!   watchdog kills the agent's whole process group, descendants included.

#[cfg(target_os = "linux")]
pub(crate) use linux::{Watchdog, arm_child};

#[cfg(not(target_os = "linux"))]
pub(crate) use other::{Watchdog, arm_child};

#[cfg(target_os = "linux")]
mod linux {
    use std::os::fd::OwnedFd;
    use std::process::Stdio;

    use rustix::pipe::{PipeFlags, pipe_with};
    use rustix::process::{Pid, Signal};
    use tokio::process::Command;

    /// The shell that watches over one agent group; `$1` is the group id. A line
    /// on stdin means "stand down" (the agent was shut down normally); end of
    /// file means Yhtye is gone, so the group is killed.
    const WATCHDOG_SCRIPT: &str =
        r#"if read -r _stand_down; then exit 0; fi; kill -s KILL -- "-$1""#;
    /// Absolute: the watchdog runs with an empty environment.
    const WATCHDOG_SHELL: &str = "/bin/sh";
    /// What Yhtye writes to stand the watchdog down.
    const STAND_DOWN: &[u8] = b"\n";

    /// Makes the kernel SIGKILL the launcher when Yhtye's spawning thread ends.
    ///
    /// The signal follows the *thread* that called `spawn`. Tokio's worker
    /// threads live as long as the runtime, which is the lifetime Yhtye needs
    /// (agents are not started from `spawn_blocking`).
    pub(crate) fn arm_child(cmd: &mut Command) {
        let parent = rustix::process::getpid();
        // SAFETY: the closure runs between fork and exec and only makes two
        // syscalls through rustix (prctl, getppid): no allocation, no locks.
        unsafe {
            cmd.pre_exec(move || {
                rustix::process::set_parent_process_death_signal(Some(Signal::KILL))?;
                // Yhtye may have died before the signal was armed: nobody would
                // deliver it then, so give up instead of starting an orphan.
                if rustix::process::getppid() != Some(parent) {
                    return Err(std::io::Error::other(
                        "Yhtye exited while the agent was starting",
                    ));
                }
                Ok(())
            });
        }
    }

    /// Kills an agent's process group once Yhtye dies without having shut the
    /// agent down. Dropping it stands the watchdog down.
    #[derive(Debug)]
    pub(crate) struct Watchdog {
        /// Write end of the pipe the watchdog reads (close-on-exec, so no agent
        /// inherits it and keeps the pipe open past Yhtye).
        stand_down: OwnedFd,
    }

    impl Watchdog {
        /// Starts the watchdog of group `pgid`. `None` when it cannot start
        /// (logged): the agent then relies on [`arm_child`] and the next sweep.
        pub(crate) fn start(pgid: Pid) -> Option<Self> {
            let (read_end, write_end) = match pipe_with(PipeFlags::CLOEXEC) {
                Ok(ends) => ends,
                Err(e) => {
                    tracing::warn!(
                        "no watchdog for agent group {}: pipe: {e}",
                        pgid.as_raw_nonzero()
                    );
                    return None;
                }
            };
            let spawned = Command::new(WATCHDOG_SHELL)
                .arg("-c")
                .arg(WATCHDOG_SCRIPT)
                .arg("yhtye-watchdog")
                .arg(pgid.as_raw_nonzero().to_string())
                .env_clear()
                .stdin(Stdio::from(read_end))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                // Its own group: the kill of the agent's group must not reach it.
                .process_group(0)
                .kill_on_drop(false)
                .spawn();
            match spawned {
                // Dropped: tokio reaps it when it exits.
                Ok(_child) => Some(Self {
                    stand_down: write_end,
                }),
                Err(e) => {
                    tracing::warn!(
                        "no watchdog for agent group {}: cannot start {WATCHDOG_SHELL}: {e}",
                        pgid.as_raw_nonzero()
                    );
                    None
                }
            }
        }
    }

    impl Drop for Watchdog {
        fn drop(&mut self) {
            match rustix::io::write(&self.stand_down, STAND_DOWN) {
                Ok(_) => {}
                // The watchdog is gone already: nothing left to stand down.
                Err(e) if e == rustix::io::Errno::PIPE => {}
                // Closing the pipe right after makes the watchdog kill the group.
                Err(e) => tracing::warn!("cannot stand the agent watchdog down: {e}"),
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod other {
    use rustix::process::Pid;
    use tokio::process::Command;

    pub(crate) fn arm_child(_cmd: &mut Command) {}

    #[derive(Debug)]
    pub(crate) struct Watchdog;

    impl Watchdog {
        pub(crate) fn start(_pgid: Pid) -> Option<Self> {
            None
        }
    }
}
