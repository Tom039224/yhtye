//! Finds the Claude Code and Codex log files and parses those that changed
//! since the last scan, in parallel. A parsed file is remembered by
//! (path, size, modification time): a repeated scan only lists and stats the
//! files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::UNIX_EPOCH;

use super::record::{Harness, ParsedFile};
use super::sources::StatsSources;
use super::{claude, codex};

/// Directories deeper than this below a log directory are not entered (Claude:
/// `<project>/<session>/subagents/` is the deepest layout; Codex: `YYYY/MM/DD`).
const MAX_DEPTH: usize = 6;
/// Worker threads parsing at the same time at most.
const MAX_WORKERS: usize = 8;
/// A file whose last modification is older than the period by more than this
/// cannot hold a response of it (its last line is at most as new as the file).
const MTIME_SLACK_MS: i64 = 86_400_000;

/// What identifies a version of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub size: u64,
    /// ms since the Unix epoch; `i64::MAX` when the file system does not say.
    pub mtime_ms: i64,
}

impl FileStamp {
    pub fn of(meta: &std::fs::Metadata) -> Self {
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(i64::MAX);
        Self {
            size: meta.len(),
            mtime_ms,
        }
    }
}

/// The parsed files, by path.
#[derive(Default)]
pub struct FileCache {
    entries: HashMap<PathBuf, (FileStamp, Arc<ParsedFile>)>,
}

impl FileCache {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// What a scan found of one harness.
#[derive(Debug, Clone, Copy, Default)]
pub struct SourceState {
    /// The log directory exists.
    pub found: bool,
    /// Files of the period (parsed now or remembered).
    pub files: u64,
}

pub struct Scan {
    pub files: Vec<(Harness, Arc<ParsedFile>)>,
    pub claude: SourceState,
    pub codex: SourceState,
    /// Files parsed by this scan (the others came from the cache).
    pub parsed: usize,
}

struct Candidate {
    path: PathBuf,
    harness: Harness,
    stamp: FileStamp,
}

/// Every `.jsonl` file below `dir` that `wanted` accepts, with its stamp.
fn list(dir: &Path, harness: Harness, wanted: fn(&str) -> bool, out: &mut Vec<Candidate>) -> bool {
    let mut stack = vec![(dir.to_path_buf(), 0_usize)];
    let mut exists = false;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        exists = true;
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let name = entry.file_name();
            if !name.to_str().is_some_and(wanted) {
                continue;
            }
            // A symlink to a file is followed; the stamp is the target's.
            if let Ok(meta) = std::fs::metadata(&path)
                && meta.is_file()
            {
                out.push(Candidate {
                    path,
                    harness,
                    stamp: FileStamp::of(&meta),
                });
            }
        }
    }
    exists
}

fn is_claude_log(name: &str) -> bool {
    name.ends_with(".jsonl")
}

fn is_codex_rollout(name: &str) -> bool {
    name.starts_with("rollout-") && name.ends_with(".jsonl")
}

fn parse_candidate(c: &Candidate) -> Option<Arc<ParsedFile>> {
    let parsed = match c.harness {
        Harness::Codex => codex::parse_file(&c.path),
        _ => claude::parse_file(&c.path),
    };
    match parsed {
        Ok(file) => Some(Arc::new(file)),
        Err(e) => {
            tracing::debug!("usage statistics: could not read {}: {e}", c.path.display());
            None
        }
    }
}

/// Parses `todo` on up to [`MAX_WORKERS`] threads.
fn parse_all(todo: &[Candidate]) -> Vec<(usize, Arc<ParsedFile>)> {
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(MAX_WORKERS)
        .min(todo.len());
    let next = AtomicUsize::new(0);
    let done = Mutex::new(Vec::with_capacity(todo.len()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(candidate) = todo.get(i) else {
                        break;
                    };
                    if let Some(file) = parse_candidate(candidate) {
                        done.lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push((i, file));
                    }
                }
            });
        }
    });
    done.into_inner().unwrap_or_else(PoisonError::into_inner)
}

/// Lists the log files of Claude Code and Codex, and parses the ones of the
/// period (modified since `since_ms`) that are not in `cache` in this version.
pub fn scan(sources: &StatsSources, cache: &Mutex<FileCache>, since_ms: i64) -> Scan {
    let mut candidates = Vec::new();
    let claude_found = sources
        .claude_projects
        .as_deref()
        .is_some_and(|dir| list(dir, Harness::ClaudeCode, is_claude_log, &mut candidates));
    let mut codex_found = false;
    for dir in &sources.codex_dirs {
        codex_found |= list(dir, Harness::Codex, is_codex_rollout, &mut candidates);
    }
    let listed: HashSet<&Path> = candidates.iter().map(|c| c.path.as_path()).collect();
    let cutoff = since_ms.saturating_sub(MTIME_SLACK_MS);
    let mut guard = cache.lock().unwrap_or_else(PoisonError::into_inner);
    guard
        .entries
        .retain(|path, _| listed.contains(path.as_path()));
    let in_period: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| c.stamp.mtime_ms >= cutoff)
        .collect();
    let mut files = Vec::with_capacity(in_period.len());
    let mut todo: Vec<Candidate> = Vec::new();
    for c in &in_period {
        match guard.entries.get(&c.path) {
            Some((stamp, file)) if *stamp == c.stamp => {
                files.push((c.harness, Arc::clone(file)));
            }
            _ => todo.push(Candidate {
                path: c.path.clone(),
                harness: c.harness,
                stamp: c.stamp,
            }),
        }
    }
    // Parsing takes long; the cache is not held meanwhile.
    drop(guard);
    let parsed = parse_all(&todo);
    let count = parsed.len();
    let mut guard = cache.lock().unwrap_or_else(PoisonError::into_inner);
    for (i, file) in parsed {
        let c = &todo[i];
        guard
            .entries
            .insert(c.path.clone(), (c.stamp, Arc::clone(&file)));
        files.push((c.harness, file));
    }
    drop(guard);

    let state = |harness: Harness, found: bool| SourceState {
        found,
        files: files.iter().filter(|(h, _)| *h == harness).count() as u64,
    };
    Scan {
        claude: state(Harness::ClaudeCode, claude_found),
        codex: state(Harness::Codex, codex_found),
        files,
        parsed: count,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn claude_line(id: &str, ts: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","cwd":"/w","sessionId":"s","message":{{"id":"{id}","model":"m","usage":{{"output_tokens":5}}}}}}"#
        ) + "\n"
    }

    fn old(path: &Path) {
        let t = SystemTime::now() - Duration::from_secs(400 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    #[test]
    fn files_are_listed_below_the_directories_parsed_once_and_reparsed_when_they_change() {
        let tmp = tempfile::tempdir().unwrap();
        let claude_dir = tmp.path().join("projects");
        let codex_dir = tmp.path().join("sessions");
        write(
            &claude_dir.join("-w/s.jsonl"),
            &claude_line("msg_1", "2026-10-01T10:00:00Z"),
        );
        write(
            &claude_dir.join("-w/s/subagents/agent-1.jsonl"),
            &claude_line("msg_2", "2026-10-01T10:00:01Z"),
        );
        write(&claude_dir.join("-w/notes.txt"), "not a log");
        write(&codex_dir.join("2026/10/01/rollout-a.jsonl"), "");
        write(&codex_dir.join("2026/10/01/other.jsonl"), "");
        let sources = StatsSources {
            claude_projects: Some(claude_dir.clone()),
            codex_dirs: vec![codex_dir, tmp.path().join("archived_sessions")],
            opencode_db: None,
        };
        let cache = Mutex::new(FileCache::default());

        let first = scan(&sources, &cache, 0);
        assert_eq!((first.claude.found, first.claude.files), (true, 2));
        assert_eq!((first.codex.found, first.codex.files), (true, 1));
        assert_eq!(first.parsed, 3);
        let records: usize = first.files.iter().map(|(_, f)| f.records.len()).sum();
        assert_eq!(records, 2);

        let second = scan(&sources, &cache, 0);
        assert_eq!(second.parsed, 0, "unchanged files come from the cache");
        assert_eq!(second.files.len(), 3);

        write(
            &claude_dir.join("-w/s.jsonl"),
            &(claude_line("msg_1", "2026-10-01T10:00:00Z")
                + &claude_line("msg_3", "2026-10-01T10:00:05Z")),
        );
        let third = scan(&sources, &cache, 0);
        assert_eq!(third.parsed, 1);
        let records: usize = third.files.iter().map(|(_, f)| f.records.len()).sum();
        assert_eq!(records, 3);

        std::fs::remove_file(claude_dir.join("-w/s.jsonl")).unwrap();
        let fourth = scan(&sources, &cache, 0);
        assert_eq!(fourth.claude.files, 1);
        assert_eq!(
            cache.lock().unwrap().len(),
            2,
            "vanished files are forgotten"
        );
    }

    #[test]
    fn files_not_modified_since_the_period_began_are_not_read() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("projects");
        let stale = dir.join("-w/stale.jsonl");
        write(&stale, &claude_line("msg_1", "2025-01-01T10:00:00Z"));
        old(&stale);
        write(
            &dir.join("-w/fresh.jsonl"),
            &claude_line("msg_2", "2026-10-01T10:00:00Z"),
        );
        let sources = StatsSources {
            claude_projects: Some(dir),
            ..StatsSources::none()
        };
        let cache = Mutex::new(FileCache::default());
        let since = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
            - 30 * 86_400_000;
        let recent = scan(&sources, &cache, since);
        assert_eq!(recent.files.len(), 1);
        assert_eq!(recent.parsed, 1);
        // A longer period needs the old one too.
        let all = scan(&sources, &cache, 0);
        assert_eq!(all.files.len(), 2);
        assert_eq!(all.parsed, 1);
    }

    #[test]
    fn missing_directories_are_not_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let sources = StatsSources {
            claude_projects: Some(tmp.path().join("nope")),
            codex_dirs: vec![tmp.path().join("nope-either")],
            opencode_db: None,
        };
        let s = scan(&sources, &Mutex::new(FileCache::default()), 0);
        assert!(!s.claude.found && !s.codex.found && s.files.is_empty());
    }
}
