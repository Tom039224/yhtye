//! The answer of `get_usage_stats` (see [`super`]).

use schemars::JsonSchema;
use serde::Serialize;
use ts_rs::TS;

use super::record::Tokens;

/// Token counts. `total` is `input + output + cache_read + cache_creation`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct StatsTokens {
    /// Input that was not served from the cache.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub total: u64,
}

impl From<Tokens> for StatsTokens {
    fn from(t: Tokens) -> Self {
        Self {
            input: t.input,
            output: t.output,
            cache_read: t.cache_read,
            cache_creation: t.cache_creation,
            total: t.total(),
        }
    }
}

/// The whole period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct StatsTotals {
    pub tokens: StatsTokens,
    /// Sessions with at least one response in the period.
    pub sessions: u64,
}

/// What one harness used on one day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct StatsDayHarness {
    /// `claude-code`, `codex` or `opencode`.
    pub harness: String,
    /// Total tokens (all four kinds).
    pub tokens: u64,
    pub sessions: u64,
}

/// One calendar day of the client's time zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct StatsDay {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub tokens: StatsTokens,
    /// Sessions with a response that day (one that runs past midnight counts on both days).
    pub sessions: u64,
    /// The harnesses that were used that day, by id; empty for a day without usage.
    pub harnesses: Vec<StatsDayHarness>,
}

/// A model or a harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct StatsBucket {
    /// The model name as the agent logged it, or the harness id. `other` is
    /// the sum of the entries cut from a long list (and `other` is `true`).
    pub name: String,
    pub tokens: StatsTokens,
    pub sessions: u64,
    pub other: bool,
}

/// A project: a registered Yhtye project (its main clone or one of its
/// worktrees) or, for work outside any, the last directory name of the
/// session's working directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct StatsProject {
    /// The project's name, or the directory name.
    pub name: String,
    /// The id of the registered project; `None` for an unregistered directory.
    pub project: Option<String>,
    pub tokens: StatsTokens,
    pub sessions: u64,
    /// The sum of the entries cut from a long list.
    pub other: bool,
}

/// A harness's logs: whether they exist on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct StatsSource {
    pub harness: String,
    /// The logs (directory or database) exist.
    pub found: bool,
    /// Log files read (1 for OpenCode's database); files older than the period are not read.
    pub files: u64,
}

/// Token usage of the coding agents on the machine the core runs on, read
/// from their own session logs (every session, not only those of Yhtye).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct UsageStats {
    /// When it was computed (ms since the Unix epoch).
    pub generated_at_ms: u64,
    /// The length of the period in days, ending today.
    pub period_days: u32,
    /// The UTC offset in minutes the days were cut with.
    pub tz_offset_min: i32,
    pub sources: Vec<StatsSource>,
    pub totals: StatsTotals,
    /// One entry per day of the period, oldest first, days without usage included.
    pub days: Vec<StatsDay>,
    /// Most tokens first; at most 20, then an `other` entry.
    pub by_model: Vec<StatsBucket>,
    /// Most tokens first.
    pub by_harness: Vec<StatsBucket>,
    /// Most tokens first; at most 20, then an `other` entry.
    pub by_project: Vec<StatsProject>,
}
