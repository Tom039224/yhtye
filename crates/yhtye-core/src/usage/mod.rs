//! Subscription usage / quota for the status bar (Stage 6b, `core-design.md` §14).
//!
//! Source: Claude Code's local `/usage` command, run in a short-lived ACP
//! session of the pinned `claude-agent-acp` adapter ([`probe`]). The adapter
//! answers it with Markdown rendered from the Agent SDK's structured usage
//! response (`usage-markdown.js`: plan name, 5-hour and weekly windows with
//! utilization and reset time). `/usage` is a local command: it does not call
//! the model. [`parse_usage_markdown`] reads that Markdown; anything it cannot
//! read is left out — values are never guessed.

mod parse;
mod probe;

use serde::Serialize;
use ts_rs::TS;

pub use parse::parse_usage_markdown;
pub use probe::{UsageError, UsageService, probe_usage};

/// The subscription usage reported by the harness.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct UsageReport {
    /// Subscription plan as Claude Code names it (`max`, `pro`, ...), if reported.
    pub plan: Option<String>,
    /// Limit windows in the order reported (5-hour, weekly, per-model weekly).
    pub windows: Vec<UsageWindow>,
    /// When the harness was asked (ms since the Unix epoch).
    pub fetched_at_ms: u64,
}

impl UsageReport {
    /// The first window of `kind`.
    #[must_use]
    pub fn window(&self, kind: UsageWindowKind) -> Option<&UsageWindow> {
        self.windows.iter().find(|w| w.kind == kind)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct UsageWindow {
    pub kind: UsageWindowKind,
    /// Label as reported (`5-hour limit`, `Weekly · all models`, `Weekly · Opus`).
    pub label: String,
    /// Utilization in percent (0–100).
    pub percent: f64,
    /// When the window resets (ms since the Unix epoch), if it could be read.
    pub resets_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum UsageWindowKind {
    /// The rolling 5-hour limit.
    FiveHour,
    /// The weekly limit across all models.
    Week,
    /// Any other window (per-model weekly limits).
    Other,
}
