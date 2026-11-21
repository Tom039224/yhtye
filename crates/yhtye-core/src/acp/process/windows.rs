//! Windows side of the agent process tree: the agent and everything it starts live
//! in one Job Object, which Yhtye alone holds.
//!
//! - **Yhtye dies** (crash, kill, power-off of the GUI): the job has
//!   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so when the OS closes Yhtye's last handle
//!   to it the whole tree is killed. This is what `parent_death` does on Linux, and
//!   it is also why there is nothing for `orphan` to sweep at the next start.
//! - **Normal stop**: stdin EOF (the caller) → a grace period → `TerminateJobObject`
//!   on the whole tree (see [`GroupKillGuard::kill`]).
//!
//! There is no polite signal in between (SIGTERM on Unix): `CTRL_BREAK_EVENT` can
//! only reach a process that shares Yhtye's console, but the agent is started with
//! `CREATE_NO_WINDOW` and so has a hidden console of its own, and attaching to it
//! (`AttachConsole`) would change a console state that belongs to the whole Yhtye
//! process.
//!
//! The agent is started suspended and only resumed once it is in the job, so it can
//! not start a child that escapes (a job is inherited by new children, never by
//! processes that already exist).

use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::ExitStatus;
use std::ptr::null;

use tokio::process::{Child, Command};
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
    TerminateProcess,
};

/// Exit code given to what a stop kills (the one `Child::kill` uses as well).
const KILLED_EXIT_CODE: u32 = 1;

/// High bits of an `NTSTATUS` error code (`0xC000_0005` access violation, …): how
/// Windows reports a process that crashed instead of exiting.
const NTSTATUS_ERROR_MASK: u32 = 0xF000_0000;
const NTSTATUS_ERROR: u32 = 0xC000_0000;

/// Starts the agent without a console window, suspended: [`GroupKillGuard::new`]
/// puts it into its job and resumes it.
pub(crate) fn configure_command(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
}

/// A Job Object that kills its processes when the last handle to it is closed.
struct Job(OwnedHandle);

impl Job {
    fn new() -> io::Result<Self> {
        // SAFETY: no attributes and no name; the returned handle (null on failure)
        // is exclusively ours.
        let raw = unsafe { CreateJobObjectW(null(), null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw` is a valid handle that nothing else owns.
        let job = Self(unsafe { OwnedHandle::from_raw_handle(raw) });
        // SAFETY: an all-zero limit struct is valid (no limit set).
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` is a live struct of the size passed with it.
        let ok = unsafe {
            SetInformationJobObject(
                job.raw(),
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn raw(&self) -> HANDLE {
        self.0.as_raw_handle()
    }

    fn assign(&self, process: HANDLE) -> io::Result<()> {
        // SAFETY: both handles are valid for the call.
        if unsafe { AssignProcessToJobObject(self.raw(), process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Kills every process in the job. Fails once the job is empty and over.
    fn terminate(&self) {
        // SAFETY: the handle is valid; the error (job already empty) is expected.
        let _ = unsafe { TerminateJobObject(self.raw(), KILLED_EXIT_CODE) };
    }
}

/// Resumes the (only) thread of a process started with `CREATE_SUSPENDED`, which
/// the standard library does not hand out.
fn resume_process(pid: u32) -> io::Result<()> {
    // SAFETY: a plain snapshot call; the handle is owned (and closed) below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: valid and owned by nobody else.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    // SAFETY: an all-zero entry is valid; `dwSize` is set as the API requires.
    let mut entry: THREADENTRY32 = unsafe { zeroed() };
    entry.dwSize = size_of::<THREADENTRY32>() as u32;
    let mut resumed = false;
    // SAFETY: `snapshot` and `entry` outlive the calls.
    let mut more = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: plain call; the handle is owned and closed right after.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: valid and owned by nobody else.
            let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
            // SAFETY: the handle has `THREAD_SUSPEND_RESUME`.
            if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            resumed = true;
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } != 0;
    }
    if resumed {
        Ok(())
    } else {
        Err(io::Error::other(
            "the suspended agent has no thread to resume",
        ))
    }
}

/// Stands for the agent's whole process tree: closing it (drop) or [`kill`]ing it
/// ends every process in it.
///
/// [`kill`]: Self::kill
pub(crate) struct GroupKillGuard {
    /// `None` when no job could be set up: the launcher alone is then stopped
    /// through its own handle (`terminate` falls back to `Child::start_kill`).
    job: Option<Job>,
}

impl GroupKillGuard {
    /// Puts the freshly spawned, still suspended `child` into a new job and lets it
    /// run. A job that can not be set up is logged and not fatal; a `child` that can
    /// not be resumed is killed and reported (it would hang forever).
    pub(crate) fn new(child: &Child, command: &str) -> io::Result<Self> {
        let job = Job::new()
            .and_then(|job| {
                let process = child
                    .raw_handle()
                    .ok_or_else(|| io::Error::other("the agent already exited"))?;
                job.assign(process)?;
                Ok(job)
            })
            .inspect_err(|e| {
                tracing::warn!(
                    "no job object for agent `{command}`, only the launcher is managed: {e}"
                );
            })
            .ok();
        let resumed = child
            .id()
            .ok_or_else(|| io::Error::other("the agent already exited"))
            .and_then(resume_process);
        if let Err(e) = resumed {
            match (&job, child.raw_handle()) {
                (Some(job), _) => job.terminate(),
                // SAFETY: the process handle is valid while `child` is.
                (None, Some(process)) => {
                    let _ = unsafe { TerminateProcess(process, KILLED_EXIT_CODE) };
                }
                (None, None) => {}
            }
            return Err(e);
        }
        Ok(Self { job })
    }

    /// Nothing to ask for: see the module docs. The caller's grace period before
    /// [`kill`](Self::kill) still applies.
    pub(crate) fn request_stop(&self) {}

    /// Kills the whole tree (`TerminateJobObject`).
    pub(crate) fn kill(&self) {
        if let Some(job) = &self.job {
            job.terminate();
        }
    }

    /// Kills whatever is left of the tree (a launcher may exit while descendants
    /// keep running) and lets go of the job.
    pub(crate) fn disarm(&mut self) {
        self.kill();
        self.job = None;
    }
}

/// `(exit code, terminating signal)` of a status. Windows has no signals; a process
/// that crashed (an `NTSTATUS` error such as an access violation, or
/// `STATUS_CONTROL_C_EXIT`) is the closest thing and is reported in the second slot
/// with its status as the value.
pub(crate) fn exit_parts(status: Option<ExitStatus>) -> (Option<i32>, Option<i32>) {
    let Some(code) = status.and_then(|s| s.code()) else {
        return (None, None);
    };
    if code as u32 & NTSTATUS_ERROR_MASK == NTSTATUS_ERROR {
        (None, Some(code))
    } else {
        (Some(code), None)
    }
}

#[cfg(test)]
mod tests {
    use std::process::Stdio;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList, QueryInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    use super::*;
    use crate::acp::process::terminate;

    const WAIT: Duration = Duration::from_secs(10);

    impl Job {
        /// Process ids currently in the job.
        fn process_ids(&self) -> Vec<u32> {
            // Two `u32` counts, then (at the next `usize` boundary) the ids; room for 64.
            let ids_at = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList)
                / size_of::<usize>();
            let mut buffer = [0usize; 4 + 64];
            // SAFETY: the buffer is big enough for the size passed and aligned like
            // the struct (it is made of `usize`).
            let ok = unsafe {
                QueryInformationJobObject(
                    self.raw(),
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr().cast(),
                    size_of_val(&buffer) as u32,
                    std::ptr::null_mut(),
                )
            };
            assert_ne!(ok, 0, "query the job: {}", io::Error::last_os_error());
            let list = buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
            // SAFETY: filled in by the call above.
            let count = unsafe { (*list).NumberOfProcessIdsInList } as usize;
            buffer[ids_at..ids_at + count.min(64)]
                .iter()
                .map(|&id| id as u32)
                .collect()
        }
    }

    impl GroupKillGuard {
        fn process_ids(&self) -> Vec<u32> {
            self.job.as_ref().expect("a job").process_ids()
        }
    }

    fn process_gone(pid: u32) -> bool {
        // SAFETY: plain calls; the handle is closed below.
        unsafe {
            let handle = OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            );
            if handle.is_null() {
                return true;
            }
            let gone = WaitForSingleObject(handle, 0) == WAIT_OBJECT_0;
            CloseHandle(handle);
            gone
        }
    }

    async fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = tokio::time::Instant::now() + WAIT;
        while !done() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {what}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// `cmd.exe` running `ping` for ten minutes: a launcher with a descendant,
    /// started the way the agents are.
    fn spawn_launcher(extra_args: &[&str]) -> (Child, GroupKillGuard) {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/c"])
            .args(extra_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        configure_command(&mut cmd);
        let child = cmd.spawn().expect("spawn cmd.exe");
        let guard = GroupKillGuard::new(&child, "cmd.exe").expect("guard");
        (child, guard)
    }

    #[tokio::test]
    async fn dropping_the_guard_ends_the_launcher_and_its_descendants() {
        let (child, guard) = spawn_launcher(&["ping", "-n", "600", "127.0.0.1"]);
        let launcher = child.id().expect("pid");
        wait_until("the descendant to appear in the job", || {
            guard.process_ids().len() >= 2
        })
        .await;
        let members = guard.process_ids();
        assert!(members.contains(&launcher));

        // No `kill`, no `terminate`: closing the handle alone ends the tree, which
        // is what the OS does for a Yhtye that died.
        drop(guard);

        wait_until("the tree to be gone", || {
            members.iter().all(|&p| process_gone(p))
        })
        .await;
    }

    #[tokio::test]
    async fn stopping_kills_a_launcher_that_ignores_stdin_eof_and_its_descendants() {
        let (mut child, mut guard) = spawn_launcher(&["ping", "-n", "600", "127.0.0.1"]);
        wait_until("the descendant to appear in the job", || {
            guard.process_ids().len() >= 2
        })
        .await;
        let members = guard.process_ids();
        // What the actor does at the end: the pipe closes, then `terminate`.
        drop(child.stdin.take());

        let status = terminate(&mut child, &mut guard).await;

        assert!(status.is_some(), "the launcher was reaped");
        // The job's termination reaches the descendants a moment after the launcher.
        wait_until("the tree to be gone", || {
            members.iter().all(|&p| process_gone(p))
        })
        .await;
    }

    #[tokio::test]
    async fn stopping_kills_descendants_that_outlive_their_launcher() {
        // `start /b` leaves ping running after cmd.exe is done.
        let (mut child, mut guard) =
            spawn_launcher(&["start", "/b", "ping", "-n", "600", "127.0.0.1"]);
        let launcher = child.id().expect("pid");
        let mut descendant = None;
        wait_until("the descendant to appear in the job", || {
            descendant = guard.process_ids().into_iter().find(|&p| p != launcher);
            descendant.is_some()
        })
        .await;
        let descendant = descendant.expect("descendant");
        drop(child.stdin.take());

        let status = terminate(&mut child, &mut guard).await;

        assert!(status.is_some(), "the launcher was reaped");
        wait_until("the descendant to be gone", || process_gone(descendant)).await;
    }

    #[test]
    fn a_crash_status_is_reported_as_a_termination_and_a_plain_code_as_an_exit() {
        use std::os::windows::process::ExitStatusExt;
        let exit = |code: u32| exit_parts(Some(ExitStatus::from_raw(code)));
        assert_eq!(exit(0), (Some(0), None));
        assert_eq!(exit(3), (Some(3), None));
        assert_eq!(exit(0xC000_0005), (None, Some(0xC000_0005_u32 as i32)));
        assert_eq!(exit_parts(None), (None, None));
    }
}
