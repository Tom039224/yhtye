//! Token usage statistics of the coding agents on the host (`get_usage_stats`):
//! how much each used per day, model, harness and project. They are computed
//! from the agents' own session logs on this machine — every session, not only
//! those Yhtye started — so a remote client sees the host's numbers.
//!
//! Not to be confused with [`crate::usage`], the plan usage meter
//! (`get_usage`). The sources and the way they are read are modelled on
//! agentsview (<https://github.com/kenn-io/agentsview>, MIT):
//!
//! - [`claude`]: Claude Code's `~/.claude/projects/**/*.jsonl`
//! - [`codex`]: Codex's `~/.codex/sessions/**` and `archived_sessions`
//! - [`opencode`]: OpenCode's SQLite database
//!
//! A scan reads each file once: [`StatsService`] remembers every parsed file by
//! (path, size, modification time), so asking again only lists and stats the
//! files, and the app warms it up when it starts.

mod aggregate;
mod claude;
mod codex;
mod opencode;
mod record;
mod scan;
mod sources;
mod time;
mod types;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

pub use aggregate::{MAX_LISTED, ProjectRoots};
pub use record::Harness;
pub use sources::StatsSources;
pub use types::{
    StatsBucket, StatsDay, StatsDayHarness, StatsProject, StatsSource, StatsTokens, StatsTotals,
    UsageStats,
};

use aggregate::{Period, aggregate};
use record::ParsedFile;
use scan::{FileCache, FileStamp, scan};

use crate::clock::now_ms;

/// The period when `get_usage_stats` does not say.
pub const DEFAULT_PERIOD_DAYS: u32 = 30;
/// The longest period.
pub const MAX_PERIOD_DAYS: u32 = 366;
/// The largest UTC offset of a time zone in minutes (UTC+14), with room to spare.
pub const MAX_TZ_OFFSET_MIN: i32 = 24 * 60;

/// The statistics of the machine, with what it remembers of the logs it read.
pub struct StatsService {
    sources: StatsSources,
    files: Arc<Mutex<FileCache>>,
    opencode: Mutex<Option<(OpenCodeStamp, Arc<ParsedFile>)>>,
    /// Held during a whole scan: a second caller waits for the first and then
    /// finds everything remembered.
    scanning: tokio::sync::Mutex<()>,
}

/// The database and its write-ahead log (recent rows may only be in the log).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OpenCodeStamp {
    db: FileStamp,
    wal: Option<FileStamp>,
}

fn stamp_of(path: &Path) -> Option<FileStamp> {
    std::fs::metadata(path).ok().map(|m| FileStamp::of(&m))
}

impl StatsService {
    #[must_use]
    pub fn new(sources: StatsSources) -> Self {
        Self {
            sources,
            files: Arc::new(Mutex::new(FileCache::default())),
            opencode: Mutex::new(None),
            scanning: tokio::sync::Mutex::new(()),
        }
    }

    /// The statistics of the last `days` days (ending today) in the time zone
    /// `tz_offset_min` minutes ahead of UTC. `refresh` forgets everything read
    /// before. The scan runs on blocking threads.
    pub async fn get(
        &self,
        days: u32,
        tz_offset_min: i32,
        refresh: bool,
        projects: Vec<ProjectRoots>,
    ) -> UsageStats {
        let _single = self.scanning.lock().await;
        if refresh {
            self.files
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            *self.opencode.lock().unwrap_or_else(PoisonError::into_inner) = None;
        }
        let period = Period {
            days,
            tz_offset_min,
            now_ms: now_ms(),
        };
        let read = self.read(period.start_ms()).await;
        let sources = read.sources();
        tokio::task::spawn_blocking(move || aggregate(&read.files, &period, &projects, sources))
            .await
            .unwrap_or_else(|e| {
                tracing::error!("usage statistics: the aggregation failed: {e}");
                aggregate(&[], &period, &[], Vec::new())
            })
    }

    /// Reads the logs of the default period ahead of the first request (the
    /// app calls it after starting).
    pub async fn warm_up(&self) {
        let _single = self.scanning.lock().await;
        let period = Period {
            days: DEFAULT_PERIOD_DAYS,
            tz_offset_min: 0,
            now_ms: now_ms(),
        };
        let started = std::time::Instant::now();
        let read = self.read(period.start_ms()).await;
        tracing::info!(
            "usage statistics: warmed up with {} files ({} read) in {:?}",
            read.files.len(),
            read.parsed,
            started.elapsed()
        );
    }

    /// Scans the files and the database for the records since `since_ms`.
    async fn read(&self, since_ms: i64) -> Read {
        let sources = self.sources.clone();
        let cache = Arc::clone(&self.files);
        let scanned = tokio::task::spawn_blocking(move || scan(&sources, &cache, since_ms))
            .await
            .unwrap_or_else(|e| {
                tracing::error!("usage statistics: the scan failed: {e}");
                scan::Scan {
                    files: Vec::new(),
                    claude: scan::SourceState::default(),
                    codex: scan::SourceState::default(),
                    parsed: 0,
                }
            });

        let mut files = scanned.files;
        let opencode_state = self.read_opencode(&mut files).await;
        Read {
            files,
            claude: scanned.claude,
            codex: scanned.codex,
            opencode: opencode_state,
            parsed: scanned.parsed,
        }
    }

    async fn read_opencode(
        &self,
        files: &mut Vec<(Harness, Arc<ParsedFile>)>,
    ) -> scan::SourceState {
        let Some(db) = self.sources.opencode_db.as_deref() else {
            return scan::SourceState::default();
        };
        let Some(db_stamp) = stamp_of(db) else {
            return scan::SourceState::default();
        };
        let wal: PathBuf = {
            let mut name = db.as_os_str().to_owned();
            name.push("-wal");
            name.into()
        };
        let stamp = OpenCodeStamp {
            db: db_stamp,
            wal: stamp_of(&wal),
        };
        let remembered = self
            .opencode
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .filter(|(s, _)| *s == stamp)
            .map(|(_, f)| Arc::clone(f));
        let file = match remembered {
            Some(file) => file,
            None => match opencode::read(db).await {
                Ok(file) => {
                    let file = Arc::new(file);
                    *self.opencode.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some((stamp, Arc::clone(&file)));
                    file
                }
                Err(e) => {
                    tracing::warn!("usage statistics: could not read {}: {e}", db.display());
                    return scan::SourceState {
                        found: true,
                        files: 0,
                    };
                }
            },
        };
        files.push((Harness::OpenCode, file));
        scan::SourceState {
            found: true,
            files: 1,
        }
    }
}

/// The result of reading the sources.
struct Read {
    files: Vec<(Harness, Arc<ParsedFile>)>,
    claude: scan::SourceState,
    codex: scan::SourceState,
    opencode: scan::SourceState,
    /// Log files read by this scan (the others were remembered).
    parsed: usize,
}

impl Read {
    fn sources(&self) -> Vec<StatsSource> {
        [
            (Harness::ClaudeCode, self.claude),
            (Harness::Codex, self.codex),
            (Harness::OpenCode, self.opencode),
        ]
        .into_iter()
        .map(|(harness, state)| StatsSource {
            harness: harness.id().to_string(),
            found: state.found,
            files: state.files,
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn nothing_found_gives_zeros_for_every_day() {
        let tmp = tempfile::tempdir().unwrap();
        let service = StatsService::new(StatsSources {
            claude_projects: Some(tmp.path().join("projects")),
            codex_dirs: vec![tmp.path().join("sessions")],
            opencode_db: Some(tmp.path().join("opencode.db")),
        });
        let stats = service.get(7, 0, false, Vec::new()).await;
        assert_eq!(stats.period_days, 7);
        assert_eq!(stats.days.len(), 7);
        assert_eq!(stats.totals.tokens.total, 0);
        assert!(stats.by_model.is_empty() && stats.by_project.is_empty());
        assert_eq!(
            stats
                .sources
                .iter()
                .map(|s| (s.harness.as_str(), s.found))
                .collect::<Vec<_>>(),
            [
                ("claude-code", false),
                ("codex", false),
                ("opencode", false)
            ]
        );
    }

    /// The statistics of this machine's real logs, printed with the time a cold
    /// and a warm call took: `cargo test -p yhtye-core --release --lib
    /// stats::tests::real_logs -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "reads the real logs of the machine"]
    async fn real_logs_of_this_machine() {
        let service = StatsService::new(StatsSources::from_env());
        // `YHTYE_STATS_DAYS=3,30` picks other periods.
        let periods: Vec<u32> = std::env::var("YHTYE_STATS_DAYS")
            .unwrap_or_else(|_| "30,365".into())
            .split(',')
            .filter_map(|d| d.trim().parse().ok())
            .collect();
        let last = *periods.last().unwrap();
        for &days in &periods {
            for pass in ["cold", "warm"] {
                let started = std::time::Instant::now();
                let stats = service.get(days, 540, false, Vec::new()).await;
                println!(
                    "{days:>3} days, {pass}: {:?} — {} sessions, {} tokens (in {} out {} cache read {} cache write {}), sources {:?}",
                    started.elapsed(),
                    stats.totals.sessions,
                    stats.totals.tokens.total,
                    stats.totals.tokens.input,
                    stats.totals.tokens.output,
                    stats.totals.tokens.cache_read,
                    stats.totals.tokens.cache_creation,
                    stats.sources
                );
                if pass == "warm" && days == last {
                    for b in stats.by_harness.iter().chain(stats.by_model.iter().take(5)) {
                        println!(
                            "  {:<40} {:>14} tokens {:>5} sessions",
                            b.name, b.tokens.total, b.sessions
                        );
                    }
                    for p in stats.by_project.iter().take(8) {
                        println!(
                            "  project {:<32} {:>14} tokens {:>5} sessions",
                            p.name, p.tokens.total, p.sessions
                        );
                    }
                    let busiest = stats.days.iter().max_by_key(|d| d.tokens.total).unwrap();
                    println!(
                        "  busiest day {} with {} tokens",
                        busiest.date, busiest.tokens.total
                    );
                }
            }
        }
        // A refresh reads everything again.
        let started = std::time::Instant::now();
        service.get(last, 540, true, Vec::new()).await;
        println!("{last} days, refresh: {:?}", started.elapsed());
    }
}
