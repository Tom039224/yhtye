//! Agents that outlive Yhtye. Every agent process group is recorded on disk
//! while it runs; when Yhtye starts it sweeps what a previous run (killed,
//! crashed, restarted in quick succession) left behind.
//!
//! A record names the group's leader by `(pid, start time)` and its owner (the
//! Yhtye that launched it) the same way, plus the boot it belongs to. The sweep
//! kills a group only when all of that still matches (see [`judge`]), so an
//! unrelated process — or a `claude` the user started by hand, which is never in
//! one of these groups — is left alone. Identity comes from `/proc` (Linux); on
//! other systems nothing is recorded and the sweep finds nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal};
use serde::{Deserialize, Serialize};

/// Directory (under the app data directory) holding one record per agent group.
pub(crate) const REGISTRY_DIR_NAME: &str = "agent-processes";
/// Extension of a finished record (`<pgid>.json`); it is written under
/// [`TEMP_EXTENSION`] first and renamed, so a crash never leaves half a record.
const RECORD_EXTENSION: &str = "json";
const TEMP_EXTENSION: &str = "tmp";
/// Process groups below this are never signalled (0 = ours, 1 = init).
const MIN_KILLABLE_PGID: i32 = 2;
/// Time an orphaned group gets to exit after SIGTERM before SIGKILL.
const ORPHAN_TERM_GRACE: Duration = Duration::from_millis(1500);
const ORPHAN_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Where `/proc/<pid>/stat` puts the start time among the fields that follow the
/// `(comm)` field (field 22 overall; the first field after `comm` is field 3).
const STAT_START_TIME_INDEX: usize = 22 - 3;
const STAT_PGRP_INDEX: usize = 5 - 3;

#[cfg(target_os = "linux")]
const PROC_DIR: &str = "/proc";
#[cfg(target_os = "linux")]
const BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";

/// What is persisted about one running agent group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AgentRecord {
    /// The launcher (`npx`, the agent binary, ...): leader of its own group.
    pub pid: i32,
    /// Always equals `pid` (the agent is started with `process_group(0)`); kept so
    /// a record that says otherwise is recognised as invalid.
    pub pgid: i32,
    /// Start time of `pid` in clock ticks since boot: tells a reused pid apart.
    pub start_ticks: u64,
    /// Boot the ticks belong to.
    pub boot_id: String,
    /// The Yhtye process that launched the agent, and its start time.
    pub owner_pid: i32,
    pub owner_start_ticks: u64,
    /// The configured command, for log lines.
    pub command: String,
}

/// The fields of `/proc/<pid>/stat` the sweep needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcStat {
    pub pid: i32,
    pub state: char,
    pub pgrp: i32,
    pub start_ticks: u64,
}

impl ProcStat {
    /// Dead but not yet reaped (it no longer runs anything).
    pub fn is_zombie(&self) -> bool {
        matches!(self.state, 'Z' | 'X' | 'x')
    }
}

/// Parses `/proc/<pid>/stat`. The command name (field 2) is in parentheses and
/// may itself contain spaces and parentheses, so the fields after it are found
/// from the *last* closing parenthesis.
pub(crate) fn parse_proc_stat(text: &str) -> Option<ProcStat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open {
        return None;
    }
    let pid = text[..open].trim().parse().ok()?;
    let rest: Vec<&str> = text[close + 1..].split_whitespace().collect();
    Some(ProcStat {
        pid,
        state: rest.first()?.chars().next()?,
        pgrp: rest.get(STAT_PGRP_INDEX)?.parse().ok()?,
        start_ticks: rest.get(STAT_START_TIME_INDEX)?.parse().ok()?,
    })
}

/// The processes of the machine and its boot, as the sweep sees them.
#[derive(Debug, Default)]
pub(crate) struct HostSnapshot {
    pub boot_id: Option<String>,
    processes: HashMap<i32, ProcStat>,
}

impl HostSnapshot {
    pub fn new(boot_id: Option<String>, processes: impl IntoIterator<Item = ProcStat>) -> Self {
        Self {
            boot_id,
            processes: processes.into_iter().map(|p| (p.pid, p)).collect(),
        }
    }

    fn get(&self, pid: i32) -> Option<&ProcStat> {
        self.processes.get(&pid)
    }

    /// Some process still runs in group `pgid`.
    fn group_alive(&self, pgid: i32) -> bool {
        self.processes
            .values()
            .any(|p| p.pgrp == pgid && !p.is_zombie())
    }

    /// Reads every process of `/proc` (`None` boot id / empty off Linux).
    pub fn read() -> Self {
        Self::new(read_boot_id(), read_all_proc_stats())
    }
}

/// Why a record needs no kill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObsoleteReason {
    /// Written in an earlier boot: its pids mean nothing now.
    OtherBoot,
    /// Not a group we could have started (or our own): never signalled.
    InvalidGroup,
    /// The launcher's pid belongs to a newer process now: the group is long gone
    /// (a pid cannot be reused while any process still has it as its group id).
    PidReused,
    /// The launcher left its group; the record no longer describes one.
    LeftGroup,
    /// Nothing runs in the group any more.
    GroupGone,
}

/// What to do with one record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The group (or what is left of it) outlived its owner: kill it.
    Orphaned,
    /// The Yhtye that launched it still runs (another instance, or one that is
    /// still shutting down): not ours to clean up.
    OwnerAlive,
    /// Nothing to kill; the record can go.
    Obsolete(ObsoleteReason),
}

/// Decides what to do with `record` given the current processes. Pure.
/// `own_pgid` is the process group of this Yhtye, which is never killed.
pub(crate) fn judge(record: &AgentRecord, host: &HostSnapshot, own_pgid: i32) -> Verdict {
    if host.boot_id.as_deref() != Some(record.boot_id.as_str()) {
        return Verdict::Obsolete(ObsoleteReason::OtherBoot);
    }
    if record.pgid < MIN_KILLABLE_PGID || record.pid != record.pgid || record.pgid == own_pgid {
        return Verdict::Obsolete(ObsoleteReason::InvalidGroup);
    }
    let owner_alive = host
        .get(record.owner_pid)
        .is_some_and(|owner| owner.start_ticks == record.owner_start_ticks && !owner.is_zombie());
    if owner_alive {
        return Verdict::OwnerAlive;
    }
    match host.get(record.pid) {
        Some(leader) if leader.start_ticks != record.start_ticks => {
            Verdict::Obsolete(ObsoleteReason::PidReused)
        }
        Some(leader) if leader.pgrp != record.pgid => Verdict::Obsolete(ObsoleteReason::LeftGroup),
        // The launcher is gone or still the same process; what runs in its group is
        // ours either way (a group id stays reserved while any member lives).
        _ if host.group_alive(record.pgid) => Verdict::Orphaned,
        _ => Verdict::Obsolete(ObsoleteReason::GroupGone),
    }
}

// --- reading /proc ---------------------------------------------------------

#[cfg(target_os = "linux")]
pub(crate) fn read_proc_stat(pid: i32) -> Option<ProcStat> {
    let text = std::fs::read_to_string(format!("{PROC_DIR}/{pid}/stat")).ok()?;
    parse_proc_stat(&text)
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn read_proc_stat(_pid: i32) -> Option<ProcStat> {
    None
}

#[cfg(target_os = "linux")]
fn read_boot_id() -> Option<String> {
    std::fs::read_to_string(BOOT_ID_PATH)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

#[cfg(not(target_os = "linux"))]
fn read_boot_id() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn read_all_proc_stats() -> Vec<ProcStat> {
    let entries = match std::fs::read_dir(PROC_DIR) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("cannot list {PROC_DIR}: {e}");
            return Vec::new();
        }
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        // A process that exits while we scan is simply missing.
        .filter_map(read_proc_stat)
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn read_all_proc_stats() -> Vec<ProcStat> {
    Vec::new()
}

/// The command line of `pid`, for log lines (`None` when it cannot be read).
fn read_cmdline(pid: i32) -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let text = raw
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(text)
}

// --- recording -------------------------------------------------------------

/// Where records are written; set once the data directory is known.
static REGISTRY_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Records of agents launched from now on go to `dir`. A later call replaces
/// the directory (only tests start more than one core per process).
pub(crate) fn install_registry(dir: PathBuf) {
    match REGISTRY_DIR.write() {
        Ok(mut slot) => *slot = Some(dir),
        Err(poisoned) => *poisoned.into_inner() = Some(dir),
    }
}

fn registry_dir() -> Option<PathBuf> {
    match REGISTRY_DIR.read() {
        Ok(slot) => slot.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// `(pid, start ticks)` of this process, read once.
fn own_identity() -> Option<(i32, u64)> {
    static IDENTITY: OnceLock<Option<(i32, u64)>> = OnceLock::new();
    *IDENTITY.get_or_init(|| {
        let pid = i32::try_from(std::process::id()).ok()?;
        Some((pid, read_proc_stat(pid)?.start_ticks))
    })
}

/// The record file of a running agent group; the file is removed on drop.
#[derive(Debug)]
pub(crate) struct Registration {
    path: PathBuf,
}

impl Drop for Registration {
    fn drop(&mut self) {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!("cannot remove {}: {e}", self.path.display()),
        }
    }
}

/// Records the freshly started agent group led by `leader` for the sweep of a
/// later start. `None` when records are off (no registry installed, not Linux)
/// or the record could not be written (logged): the agent runs unrecorded.
pub(crate) fn register(leader: Pid, command: &str) -> Option<Registration> {
    let dir = registry_dir()?;
    let pgid = leader.as_raw_nonzero().get();
    let record = build_record(pgid, command)?;
    match write_record(&dir, &record) {
        Ok(path) => Some(Registration { path }),
        Err(e) => {
            tracing::warn!(
                "cannot record agent group {pgid} in {}: {e}; a crash of Yhtye would not be cleaned up on the next start",
                dir.display()
            );
            None
        }
    }
}

pub(super) fn build_record(pgid: i32, command: &str) -> Option<AgentRecord> {
    let (Some(boot_id), Some(leader), Some((owner_pid, owner_start_ticks))) =
        (read_boot_id(), read_proc_stat(pgid), own_identity())
    else {
        if cfg!(target_os = "linux") {
            tracing::warn!("cannot identify agent group {pgid} from /proc; it is not recorded");
        }
        return None;
    };
    Some(AgentRecord {
        pid: pgid,
        pgid,
        start_ticks: leader.start_ticks,
        boot_id,
        owner_pid,
        owner_start_ticks,
        command: command.to_owned(),
    })
}

fn record_path(dir: &Path, pgid: i32) -> PathBuf {
    dir.join(format!("{pgid}.{RECORD_EXTENSION}"))
}

pub(super) fn write_record(dir: &Path, record: &AgentRecord) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = record_path(dir, record.pgid);
    let temp = path.with_extension(format!("{RECORD_EXTENSION}.{TEMP_EXTENSION}"));
    std::fs::write(&temp, serde_json::to_vec(record)?)?;
    std::fs::rename(&temp, &path)?;
    Ok(path)
}

// --- sweeping --------------------------------------------------------------

/// What a sweep did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SweepReport {
    /// Groups of a dead Yhtye that were killed.
    pub killed: usize,
    /// Records dropped without killing anything.
    pub obsolete: usize,
    /// Records of a Yhtye that still runs, left alone.
    pub kept: usize,
}

/// Kills the agent groups that a dead Yhtye left in `dir` and removes their
/// records. Blocks for up to [`ORPHAN_TERM_GRACE`] while they exit.
pub(crate) fn sweep(dir: &Path) -> SweepReport {
    let records = load_records(dir);
    if records.is_empty() {
        return SweepReport::default();
    }
    let host = HostSnapshot::read();
    let own_pgid = rustix::process::getpgrp().as_raw_nonzero().get();
    let mut report = SweepReport::default();
    let mut orphaned = Vec::new();
    for (path, record) in records {
        match judge(&record, &host, own_pgid) {
            Verdict::Orphaned => orphaned.push((path, record)),
            Verdict::OwnerAlive => report.kept += 1,
            Verdict::Obsolete(reason) => {
                tracing::info!(
                    "agent record {} ({}) is obsolete: {reason:?}",
                    record.pgid,
                    record.command
                );
                report.obsolete += 1;
                remove_record(&path);
            }
        }
    }
    report.killed = orphaned.len();
    kill_orphans(&orphaned);
    for (path, _) in &orphaned {
        remove_record(path);
    }
    report
}

/// SIGTERM to every orphaned group, then SIGKILL to what is still there after
/// the grace period.
fn kill_orphans(orphans: &[(PathBuf, AgentRecord)]) {
    for (_, record) in orphans {
        tracing::warn!(
            "killing agent group {} ({}), left over from a previous run of Yhtye (launcher: {})",
            record.pgid,
            record.command,
            read_cmdline(record.pid).unwrap_or_else(|| "already gone".into()),
        );
        signal_group(record.pgid, Signal::TERM);
    }
    let deadline = Instant::now() + ORPHAN_TERM_GRACE;
    let mut remaining = orphans_still_running(orphans);
    while !remaining.is_empty() && Instant::now() < deadline {
        std::thread::sleep(ORPHAN_POLL_INTERVAL);
        remaining = orphans_still_running(orphans);
    }
    for pgid in remaining {
        tracing::warn!(
            "agent group {pgid} ignored SIGTERM for {ORPHAN_TERM_GRACE:?}; sending SIGKILL"
        );
        signal_group(pgid, Signal::KILL);
    }
}

/// The groups among `orphans` in which something still runs (zombies, which wait
/// for init to reap them, do not count).
fn orphans_still_running(orphans: &[(PathBuf, AgentRecord)]) -> Vec<i32> {
    let host = HostSnapshot::read();
    orphans
        .iter()
        .map(|(_, record)| record.pgid)
        .filter(|pgid| host.group_alive(*pgid))
        .collect()
}

fn signal_group(pgid: i32, signal: Signal) {
    // `judge` only returns `Orphaned` for ids above `MIN_KILLABLE_PGID`.
    let Some(pid) = Pid::from_raw(pgid).filter(|_| pgid >= MIN_KILLABLE_PGID) else {
        return;
    };
    match rustix::process::kill_process_group(pid, signal) {
        Ok(()) => {}
        // Already gone.
        Err(e) if e == rustix::io::Errno::SRCH => {}
        Err(e) => tracing::warn!("cannot signal agent group {pgid}: {e}"),
    }
}

/// Every readable record in `dir`. A record that cannot be read or parsed says
/// nothing trustworthy about any process, so it is removed.
fn load_records(dir: &Path) -> Vec<(PathBuf, AgentRecord)> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!("cannot list agent records in {}: {e}", dir.display());
            return Vec::new();
        }
    };
    let mut records = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(RECORD_EXTENSION) {
            continue;
        }
        let parsed = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<AgentRecord>(&bytes).map_err(|e| e.to_string())
            });
        match parsed {
            Ok(record) => records.push((path, record)),
            Err(e) => {
                tracing::warn!("dropping unreadable agent record {}: {e}", path.display());
                remove_record(&path);
            }
        }
    }
    records
}

fn remove_record(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("cannot remove agent record {}: {e}", path.display()),
    }
}

/// Starts recording agents under `data_dir` and kills the groups a previous run
/// left behind. Called before any agent of this run is started.
pub(crate) async fn start_registry(data_dir: &Path) {
    let dir = data_dir.join(REGISTRY_DIR_NAME);
    install_registry(dir.clone());
    match tokio::task::spawn_blocking(move || sweep(&dir)).await {
        Ok(report) if report == SweepReport::default() => {}
        Ok(report) => tracing::info!(
            "agent sweep: {} orphaned group(s) killed, {} obsolete record(s) dropped, {} kept",
            report.killed,
            report.obsolete,
            report.kept
        ),
        Err(e) => tracing::warn!("the sweep of leftover agents failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOT: &str = "boot-1";
    const OWNER: i32 = 100;
    const LEADER: i32 = 4000;
    const OWN_PGID: i32 = 90;

    fn record() -> AgentRecord {
        AgentRecord {
            pid: LEADER,
            pgid: LEADER,
            start_ticks: 5000,
            boot_id: BOOT.into(),
            owner_pid: OWNER,
            owner_start_ticks: 700,
            command: "npx".into(),
        }
    }

    fn stat(pid: i32, pgrp: i32, start_ticks: u64, state: char) -> ProcStat {
        ProcStat {
            pid,
            state,
            pgrp,
            start_ticks,
        }
    }

    fn host(processes: Vec<ProcStat>) -> HostSnapshot {
        HostSnapshot::new(Some(BOOT.into()), processes)
    }

    // --- parse_proc_stat ---

    #[test]
    fn parses_a_plain_stat_line() {
        let line = "4000 (node) S 1 4000 3999 0 -1 4194560 100 0 0 0 5 1 0 0 20 0 11 0 5000 1000000 2000 18446744073709551615";
        assert_eq!(
            parse_proc_stat(line),
            Some(ProcStat {
                pid: 4000,
                state: 'S',
                pgrp: 4000,
                start_ticks: 5000
            })
        );
    }

    #[test]
    fn parses_a_command_name_with_spaces_and_parentheses() {
        let line = "77 (npm exec (@zed) x) R 12 34 56 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 999 0 0 0";
        let parsed = parse_proc_stat(line).expect("parsed");
        assert_eq!(
            (parsed.pid, parsed.state, parsed.pgrp, parsed.start_ticks),
            (77, 'R', 34, 999)
        );
    }

    #[test]
    fn rejects_truncated_or_garbled_stat_lines() {
        assert_eq!(parse_proc_stat(""), None);
        assert_eq!(parse_proc_stat("12 (x"), None);
        assert_eq!(parse_proc_stat("12 (x) S 1 2 3"), None);
        assert_eq!(
            parse_proc_stat("abc (x) S 1 2 3 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 7"),
            None
        );
        assert_eq!(parse_proc_stat(") (x"), None);
    }

    // --- judge ---

    #[test]
    fn a_live_group_of_a_dead_owner_is_orphaned() {
        let h = host(vec![
            stat(LEADER, LEADER, 5000, 'S'),
            stat(4100, LEADER, 5100, 'S'),
        ]);
        assert_eq!(judge(&record(), &h, OWN_PGID), Verdict::Orphaned);
    }

    #[test]
    fn grandchildren_outliving_their_launcher_are_orphaned() {
        // The launcher is gone; only a node and a claude of the same group remain.
        let h = host(vec![
            stat(4100, LEADER, 5100, 'S'),
            stat(4200, LEADER, 5200, 'S'),
        ]);
        assert_eq!(judge(&record(), &h, OWN_PGID), Verdict::Orphaned);
    }

    #[test]
    fn a_running_owner_keeps_its_agents() {
        let h = host(vec![
            stat(OWNER, 80, 700, 'S'),
            stat(LEADER, LEADER, 5000, 'S'),
        ]);
        assert_eq!(judge(&record(), &h, OWN_PGID), Verdict::OwnerAlive);
    }

    #[test]
    fn a_reused_owner_pid_does_not_count_as_the_owner() {
        let h = host(vec![
            stat(OWNER, 80, 9999, 'S'),
            stat(LEADER, LEADER, 5000, 'S'),
        ]);
        assert_eq!(judge(&record(), &h, OWN_PGID), Verdict::Orphaned);
    }

    #[test]
    fn a_zombie_owner_does_not_count_as_running() {
        let h = host(vec![
            stat(OWNER, 80, 700, 'Z'),
            stat(LEADER, LEADER, 5000, 'S'),
        ]);
        assert_eq!(judge(&record(), &h, OWN_PGID), Verdict::Orphaned);
    }

    #[test]
    fn a_reused_launcher_pid_is_never_killed() {
        // An unrelated process (a `claude` the user started) got the pid since.
        let h = host(vec![stat(LEADER, LEADER, 8000, 'S')]);
        assert_eq!(
            judge(&record(), &h, OWN_PGID),
            Verdict::Obsolete(ObsoleteReason::PidReused)
        );
    }

    #[test]
    fn a_launcher_that_left_its_group_is_not_killed() {
        let h = host(vec![stat(LEADER, 31, 5000, 'S')]);
        assert_eq!(
            judge(&record(), &h, OWN_PGID),
            Verdict::Obsolete(ObsoleteReason::LeftGroup)
        );
    }

    #[test]
    fn an_empty_group_is_obsolete() {
        let none = host(vec![]);
        assert_eq!(
            judge(&record(), &none, OWN_PGID),
            Verdict::Obsolete(ObsoleteReason::GroupGone)
        );
        let only_a_zombie = host(vec![stat(LEADER, LEADER, 5000, 'Z')]);
        assert_eq!(
            judge(&record(), &only_a_zombie, OWN_PGID),
            Verdict::Obsolete(ObsoleteReason::GroupGone)
        );
    }

    #[test]
    fn a_record_of_another_boot_is_obsolete_even_if_the_pids_match() {
        let mut h = host(vec![stat(LEADER, LEADER, 5000, 'S')]);
        h.boot_id = Some("boot-2".into());
        assert_eq!(
            judge(&record(), &h, OWN_PGID),
            Verdict::Obsolete(ObsoleteReason::OtherBoot)
        );
        h.boot_id = None;
        assert_eq!(
            judge(&record(), &h, OWN_PGID),
            Verdict::Obsolete(ObsoleteReason::OtherBoot)
        );
    }

    #[test]
    fn invalid_or_dangerous_groups_are_never_signalled() {
        let h = host(vec![stat(1, 1, 1, 'S'), stat(OWN_PGID, OWN_PGID, 5, 'S')]);
        let invalid = Verdict::Obsolete(ObsoleteReason::InvalidGroup);
        for (pid, pgid) in [
            (0, 0),
            (1, 1),
            (-5, -5),
            (LEADER, LEADER + 1),
            (OWN_PGID, OWN_PGID),
        ] {
            let r = AgentRecord {
                pid,
                pgid,
                ..record()
            };
            assert_eq!(judge(&r, &h, OWN_PGID), invalid, "pid {pid} pgid {pgid}");
        }
    }

    // --- records on disk ---

    #[test]
    fn a_record_survives_a_round_trip_through_the_registry_directory() {
        let dir = tempfile::tempdir().expect("dir");
        let r = record();
        let path = write_record(dir.path(), &r).expect("written");
        assert_eq!(path, record_path(dir.path(), LEADER));
        let loaded = load_records(dir.path());
        assert_eq!(loaded, vec![(path, r)]);
        // No temporary file is left behind.
        assert_eq!(std::fs::read_dir(dir.path()).expect("list").count(), 1);
    }

    #[test]
    fn an_unreadable_record_is_dropped_and_other_files_are_ignored() {
        let dir = tempfile::tempdir().expect("dir");
        let bad = dir.path().join("12.json");
        std::fs::write(&bad, "{ not json").expect("write");
        let other = dir.path().join("notes.txt");
        std::fs::write(&other, "x").expect("write");
        assert!(load_records(dir.path()).is_empty());
        assert!(!bad.exists(), "the garbage record is removed");
        assert!(other.exists(), "unrelated files are untouched");
    }

    #[test]
    fn sweeping_a_missing_directory_does_nothing() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(sweep(&dir.path().join("absent")), SweepReport::default());
    }
}
