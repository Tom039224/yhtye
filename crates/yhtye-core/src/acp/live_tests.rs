//! Tests with real processes (Linux): the sweep of a dead Yhtye's agents, and
//! what happens to an agent group when Yhtye is killed. The latter runs this
//! test binary again as the "Yhtye" (`spawner_entry`) and SIGKILLs it.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal};

use super::config::HarnessConfig;
use super::orphan::{
    AgentRecord, HostSnapshot, ObsoleteReason, SweepReport, Verdict, build_record, judge,
    read_proc_stat, sweep, write_record,
};
use super::parent_death::arm_child;
use super::process::spawn_process;
use crate::runtime::{Core, CoreConfig};
use crate::secrets::{MemoryBackend, SecretEnv};

const WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(25);
/// A pid no process has (above the kernel's `pid_max` limit of 2^22).
const NO_SUCH_PID: i32 = i32::MAX;
/// Lifetime of the helper processes if a test fails before cleaning them up.
const HELPER_LIFETIME: &str = "120";

const SPAWNER_DIR_ENV: &str = "YHTYE_TEST_SPAWNER_DIR";
const SPAWNER_MODE_ENV: &str = "YHTYE_TEST_SPAWNER_MODE";
const MODE_FULL: &str = "full";
const MODE_PDEATHSIG_ONLY: &str = "pdeathsig-only";

/// The registry directory is process-wide state: tests that install one run one at a time.
fn registry_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(POLL);
    }
}

/// Not running any more (a zombie waits for its reaper and runs nothing).
fn is_gone(pid: i32) -> bool {
    read_proc_stat(pid).is_none_or(|s| s.is_zombie())
}

fn read_pid(path: &Path) -> Option<i32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Writes the file in one step, so a reader never sees half of it.
fn write_atomically(path: &Path, text: &str) {
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, text).expect("write");
    std::fs::rename(&temp, path).expect("rename");
}

/// Kills what a failed test leaves behind.
struct Cleanup {
    groups: Vec<i32>,
    pids: Vec<i32>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for pgid in &self.groups {
            if let Some(pid) = Pid::from_raw(*pgid) {
                let _ = rustix::process::kill_process_group(pid, Signal::KILL);
            }
        }
        for pid in &self.pids {
            if let Some(pid) = Pid::from_raw(*pid) {
                let _ = rustix::process::kill_process(pid, Signal::KILL);
            }
        }
    }
}

/// A launcher (`sh`) in its own group with a `sleep` grandchild: the shape of
/// `npx` → `node` → `claude`. Returns the launcher and the grandchild's pid.
fn spawn_group(dir: &Path) -> (Child, i32) {
    use std::os::unix::process::CommandExt;
    let pid_file = dir.join("grandchild.pid");
    let child = Command::new("sh")
        .args([
            "-c",
            &format!(
                "sleep {HELPER_LIFETIME} & echo $! > \"$0.tmp\" && mv \"$0.tmp\" \"$0\"; wait"
            ),
        ])
        .arg(&pid_file)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sh");
    let mut grandchild = None;
    wait_until("the grandchild to start", || {
        grandchild = read_pid(&pid_file);
        grandchild.is_some()
    });
    (child, grandchild.expect("grandchild pid"))
}

fn leader_pid(child: &Child) -> i32 {
    i32::try_from(child.id()).expect("pid")
}

fn record_of(child: &Child) -> AgentRecord {
    build_record(leader_pid(child), "sh").expect("record")
}

/// A record whose owner is a Yhtye that no longer exists.
fn orphaned_record_of(child: &Child) -> AgentRecord {
    AgentRecord {
        owner_pid: NO_SUCH_PID,
        ..record_of(child)
    }
}

#[test]
fn the_sweep_kills_the_whole_group_of_a_dead_owner() {
    let dir = tempfile::tempdir().expect("dir");
    let registry = dir.path().join("registry");
    let (mut launcher, grandchild) = spawn_group(dir.path());
    let pgid = leader_pid(&launcher);
    let _cleanup = Cleanup {
        groups: vec![pgid],
        pids: vec![],
    };
    let record_file = write_record(&registry, &orphaned_record_of(&launcher)).expect("record");

    let report = sweep(&registry);

    assert_eq!(
        report,
        SweepReport {
            killed: 1,
            obsolete: 0,
            kept: 0
        }
    );
    wait_until("the grandchild to die", || is_gone(grandchild));
    wait_until("the launcher to die", || is_gone(pgid));
    assert!(!record_file.exists(), "a handled record is removed");
    launcher.wait().expect("reap");
}

#[test]
fn the_sweep_kills_grandchildren_whose_launcher_is_already_gone() {
    let dir = tempfile::tempdir().expect("dir");
    let registry = dir.path().join("registry");
    let (mut launcher, grandchild) = spawn_group(dir.path());
    let pgid = leader_pid(&launcher);
    let _cleanup = Cleanup {
        groups: vec![pgid],
        pids: vec![],
    };
    let record = orphaned_record_of(&launcher);
    // Only the launcher dies (and is reaped); the sleep stays in the group.
    launcher.kill().expect("kill the launcher");
    launcher.wait().expect("reap");
    assert!(!is_gone(grandchild));
    write_record(&registry, &record).expect("record");

    let report = sweep(&registry);

    assert_eq!(report.killed, 1);
    wait_until("the grandchild to die", || is_gone(grandchild));
}

#[test]
fn the_sweep_spares_a_process_that_reuses_the_launcher_pid() {
    let dir = tempfile::tempdir().expect("dir");
    let registry = dir.path().join("registry");
    let (mut launcher, grandchild) = spawn_group(dir.path());
    let pgid = leader_pid(&launcher);
    let _cleanup = Cleanup {
        groups: vec![pgid],
        pids: vec![],
    };
    let mut record = orphaned_record_of(&launcher);
    // The record is from an earlier process that had this pid.
    record.start_ticks += 1;
    let record_file = write_record(&registry, &record).expect("record");

    let report = sweep(&registry);

    assert_eq!(
        report,
        SweepReport {
            killed: 0,
            obsolete: 1,
            kept: 0
        }
    );
    assert!(!is_gone(pgid) && !is_gone(grandchild), "nothing was killed");
    assert!(!record_file.exists());
    launcher.kill().expect("kill");
    launcher.wait().expect("reap");
}

#[test]
fn the_sweep_leaves_the_agents_of_a_running_yhtye_alone() {
    let dir = tempfile::tempdir().expect("dir");
    let registry = dir.path().join("registry");
    let (mut launcher, grandchild) = spawn_group(dir.path());
    let pgid = leader_pid(&launcher);
    let _cleanup = Cleanup {
        groups: vec![pgid],
        pids: vec![],
    };
    // `record_of` makes this test process the owner, and it is running.
    let record_file = write_record(&registry, &record_of(&launcher)).expect("record");

    let report = sweep(&registry);

    assert_eq!(
        report,
        SweepReport {
            killed: 0,
            obsolete: 0,
            kept: 1
        }
    );
    assert!(!is_gone(pgid) && !is_gone(grandchild));
    assert!(record_file.exists(), "the owner still needs its record");
    launcher.kill().expect("kill");
    launcher.wait().expect("reap");
}

#[test]
fn judging_against_the_real_process_table_agrees_with_the_snapshot_logic() {
    let dir = tempfile::tempdir().expect("dir");
    let (mut launcher, _grandchild) = spawn_group(dir.path());
    let _cleanup = Cleanup {
        groups: vec![leader_pid(&launcher)],
        pids: vec![],
    };
    let own_pgid = rustix::process::getpgrp().as_raw_nonzero().get();
    let host = HostSnapshot::read();

    assert_eq!(
        judge(&orphaned_record_of(&launcher), &host, own_pgid),
        Verdict::Orphaned
    );
    assert_eq!(
        judge(&record_of(&launcher), &host, own_pgid),
        Verdict::OwnerAlive
    );
    // This test process is neither a launcher nor in the launcher's group.
    let own = AgentRecord {
        pid: own_pgid,
        pgid: own_pgid,
        ..orphaned_record_of(&launcher)
    };
    assert_eq!(
        judge(&own, &host, own_pgid),
        Verdict::Obsolete(ObsoleteReason::InvalidGroup)
    );
    launcher.kill().expect("kill");
    launcher.wait().expect("reap");
}

// --- when Yhtye itself is killed ---

/// Plays Yhtye for [`yhtye_killed_`]-style tests: started again as a child of
/// the test (see [`run_spawner`]), it launches an agent and waits to be killed.
/// Does nothing in a normal test run.
#[test]
fn spawner_entry() {
    let Some(dir) = std::env::var_os(SPAWNER_DIR_ENV).map(PathBuf::from) else {
        return;
    };
    let mode = std::env::var(SPAWNER_MODE_ENV).unwrap_or_default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let _agent = runtime.block_on(async {
        if mode == MODE_PDEATHSIG_ONLY {
            let mut cmd = tokio::process::Command::new("sleep");
            cmd.arg(HELPER_LIFETIME).kill_on_drop(true);
            arm_child(&mut cmd);
            let child = cmd.spawn().expect("spawn sleep");
            write_atomically(&dir.join("leader.pid"), &child.id().expect("pid").to_string());
            Box::new(child) as Box<dyn std::any::Any>
        } else {
            super::orphan::install_registry(dir.join("registry"));
            let mut harness = HarnessConfig::plain(
                "sh",
                vec![
                    "-c".into(),
                    format!(
                        "sleep {HELPER_LIFETIME} & echo $! > \"$0.tmp\" && mv \"$0.tmp\" \"$0\"; wait"
                    ),
                    dir.join("grandchild.pid").display().to_string(),
                ],
            );
            harness.startup_timeout = Duration::from_secs(10);
            let (events, _rx) = tokio::sync::mpsc::unbounded_channel();
            let process =
                spawn_process(&harness, &dir, &SecretEnv::default(), events).expect("spawn");
            write_atomically(
                &dir.join("leader.pid"),
                &process.child.id().expect("pid").to_string(),
            );
            Box::new((process, _rx)) as Box<dyn std::any::Any>
        }
    });
    // The parent test kills this process long before.
    std::thread::sleep(Duration::from_secs(60));
}

/// Starts this test binary as a stand-in Yhtye that launches an agent.
/// Returns it with the agent launcher's pid (and the grandchild's, in full mode).
fn run_spawner(dir: &Path, mode: &str) -> (Child, i32, Option<i32>) {
    let child = Command::new(std::env::current_exe().expect("exe"))
        .args([
            "--exact",
            "acp::live_tests::spawner_entry",
            "--test-threads=1",
        ])
        .env(SPAWNER_DIR_ENV, dir)
        .env(SPAWNER_MODE_ENV, mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the stand-in Yhtye");
    let leader_file = dir.join("leader.pid");
    let grandchild_file = dir.join("grandchild.pid");
    let wants_grandchild = mode == MODE_FULL;
    wait_until("the stand-in Yhtye to launch the agent", || {
        read_pid(&leader_file).is_some()
            && (!wants_grandchild || read_pid(&grandchild_file).is_some())
    });
    (
        child,
        read_pid(&leader_file).expect("leader"),
        wants_grandchild.then(|| read_pid(&grandchild_file).expect("grandchild")),
    )
}

#[test]
fn killing_yhtye_ends_the_launcher_through_the_kernel() {
    let dir = tempfile::tempdir().expect("dir");
    let (mut yhtye, leader, _) = run_spawner(dir.path(), MODE_PDEATHSIG_ONLY);
    let _cleanup = Cleanup {
        groups: vec![],
        pids: vec![leader],
    };
    assert!(!is_gone(leader));

    yhtye.kill().expect("SIGKILL the stand-in Yhtye");
    yhtye.wait().expect("reap");

    wait_until("the launcher to die with its parent", || is_gone(leader));
}

#[test]
fn killing_yhtye_ends_the_whole_agent_group_and_the_next_start_clears_the_record() {
    let dir = tempfile::tempdir().expect("dir");
    let (mut yhtye, leader, grandchild) = run_spawner(dir.path(), MODE_FULL);
    let grandchild = grandchild.expect("grandchild");
    let _cleanup = Cleanup {
        groups: vec![leader],
        pids: vec![grandchild],
    };
    let registry = dir.path().join("registry");
    let record_file = registry.join(format!("{leader}.json"));
    assert!(record_file.exists(), "the running agent is recorded");
    assert!(!is_gone(leader) && !is_gone(grandchild));

    yhtye.kill().expect("SIGKILL the stand-in Yhtye");
    yhtye.wait().expect("reap");

    wait_until("the grandchild to die with Yhtye", || is_gone(grandchild));
    wait_until("the launcher to die with Yhtye", || is_gone(leader));
    assert!(
        record_file.exists(),
        "a killed Yhtye cannot remove its record"
    );
    // The next start finds nothing to kill and drops the record.
    assert_eq!(
        sweep(&registry),
        SweepReport {
            killed: 0,
            obsolete: 1,
            kept: 0
        }
    );
    assert!(!record_file.exists());
}

#[test]
fn a_normal_shutdown_removes_the_record() {
    let _lock = registry_lock();
    let dir = tempfile::tempdir().expect("dir");
    let registry = dir.path().join("registry");
    super::orphan::install_registry(registry.clone());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let harness = HarnessConfig::plain("sleep", vec![HELPER_LIFETIME.into()]);
        let (events, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut process =
            spawn_process(&harness, dir.path(), &SecretEnv::default(), events).expect("spawn");
        let pgid = process.child.id().expect("pid");
        let record_file = registry.join(format!("{pgid}.json"));
        assert!(record_file.exists());

        let status = super::process::terminate(&mut process.child, &mut process.group).await;

        assert!(status.is_some(), "the agent was reaped");
        wait_until("the agent to be gone", || is_gone(pgid as i32));

        assert!(
            !record_file.exists(),
            "the record is removed with the agent"
        );
    });
}

#[test]
fn starting_the_core_kills_the_agents_a_dead_yhtye_left_behind() {
    let _lock = registry_lock();
    let dir = tempfile::tempdir().expect("dir");
    let data_dir = dir.path().join("data");
    let (mut launcher, grandchild) = spawn_group(dir.path());
    let pgid = leader_pid(&launcher);
    let _cleanup = Cleanup {
        groups: vec![pgid],
        pids: vec![],
    };
    let registry = data_dir.join(super::orphan::REGISTRY_DIR_NAME);
    let record_file = write_record(&registry, &orphaned_record_of(&launcher)).expect("record");
    let mut cfg = CoreConfig::claude_code(&data_dir, "haiku");
    cfg.secret_backend = Arc::new(MemoryBackend::default());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(Core::start(cfg)).expect("core");

    // The sweep finished before `start` returned.
    assert!(is_gone(grandchild) && is_gone(pgid));
    assert!(!record_file.exists());
    launcher.wait().expect("reap");
}
