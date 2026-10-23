//! What the parsers produce: one [`Record`] per model response, kept per
//! source file in a [`ParsedFile`] (the unit the cache remembers).

use std::collections::HashMap;
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
use std::sync::Arc;

use crate::agents::{CLAUDE_CODE, CODEX, OPENCODE};

/// The coding agents whose logs are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Harness {
    ClaudeCode,
    Codex,
    OpenCode,
}

impl Harness {
    /// The harness id Yhtye uses elsewhere (`claude-code`, `codex`, `opencode`).
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::ClaudeCode => CLAUDE_CODE,
            Self::Codex => CODEX,
            Self::OpenCode => OPENCODE,
        }
    }
}

/// Token counts of one response (or a sum of them).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    /// Input that was not served from the cache.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

impl Tokens {
    #[must_use]
    pub fn total(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_creation)
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.total() == 0
    }

    pub fn add(&mut self, other: &Tokens) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_creation = self.cache_creation.saturating_add(other.cache_creation);
    }
}

/// One model response.
#[derive(Debug, Clone)]
pub struct Record {
    /// When it was written (ms since the Unix epoch).
    pub ts_ms: u64,
    /// Index into [`ParsedFile::sessions`].
    pub session: u32,
    /// Index into [`ParsedFile::models`].
    pub model: u32,
    /// Index into [`ParsedFile::cwds`].
    pub cwd: u32,
    /// Identifies the response across files (Claude's `message.id`): the same
    /// response can be written to several files (a resumed session copies the
    /// history), and must be counted once.
    pub key: Option<u64>,
    pub tokens: Tokens,
}

/// Everything read from one source (a log file or the OpenCode database).
#[derive(Debug, Clone, Default)]
pub struct ParsedFile {
    pub sessions: Vec<Arc<str>>,
    pub models: Vec<Arc<str>>,
    pub cwds: Vec<Arc<str>>,
    pub records: Vec<Record>,
}

/// Builds a [`ParsedFile`]: interns the strings and keeps, of the records that
/// share a key, the last one (streamed responses are written several times,
/// each snapshot cumulative).
#[derive(Default)]
pub struct FileBuilder {
    file: ParsedFile,
    session_ix: HashMap<Arc<str>, u32>,
    model_ix: HashMap<Arc<str>, u32>,
    cwd_ix: HashMap<Arc<str>, u32>,
    by_key: HashMap<u64, usize, BuildHasherDefault<DefaultHasher>>,
}

/// The cross-file key of a response id.
#[must_use]
pub fn response_key(id: &str) -> u64 {
    BuildHasherDefault::<DefaultHasher>::default().hash_one(id)
}

fn intern(index: &mut HashMap<Arc<str>, u32>, table: &mut Vec<Arc<str>>, value: &str) -> u32 {
    if let Some(&i) = index.get(value) {
        return i;
    }
    let i = u32::try_from(table.len()).unwrap_or(u32::MAX);
    let value: Arc<str> = Arc::from(value);
    table.push(Arc::clone(&value));
    index.insert(value, i);
    i
}

impl FileBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(
        &mut self,
        session: &str,
        model: &str,
        cwd: &str,
        ts_ms: u64,
        key: Option<u64>,
        tokens: Tokens,
    ) {
        let record = Record {
            ts_ms,
            session: intern(&mut self.session_ix, &mut self.file.sessions, session),
            model: intern(&mut self.model_ix, &mut self.file.models, model),
            cwd: intern(&mut self.cwd_ix, &mut self.file.cwds, cwd),
            key,
            tokens,
        };
        match key.and_then(|k| self.by_key.get(&k).copied()) {
            Some(at) => self.file.records[at] = record,
            None => {
                if let Some(k) = key {
                    self.by_key.insert(k, self.file.records.len());
                }
                self.file.records.push(record);
            }
        }
    }

    #[must_use]
    pub fn finish(mut self) -> ParsedFile {
        self.file.records.shrink_to_fit();
        self.file
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(output: u64) -> Tokens {
        Tokens {
            output,
            ..Tokens::default()
        }
    }

    #[test]
    fn the_last_snapshot_of_a_response_wins() {
        let mut b = FileBuilder::new();
        let key = Some(response_key("msg_1"));
        b.push("s", "m", "/w", 1, key, tokens(5));
        b.push("s", "m", "/w", 2, Some(response_key("msg_2")), tokens(7));
        b.push("s", "m", "/w", 3, key, tokens(9));
        let f = b.finish();
        assert_eq!(f.records.len(), 2);
        assert_eq!(f.records[0].tokens.output, 9);
        assert_eq!(f.records[0].ts_ms, 3);
        assert_eq!(f.records[1].tokens.output, 7);
        assert_eq!(f.sessions.len(), 1);
    }

    #[test]
    fn records_without_a_key_are_all_kept() {
        let mut b = FileBuilder::new();
        b.push("s", "m", "/w", 1, None, tokens(1));
        b.push("s", "m", "/w", 1, None, tokens(1));
        assert_eq!(b.finish().records.len(), 2);
    }

    #[test]
    fn totals_add_the_four_kinds() {
        let t = Tokens {
            input: 1,
            output: 2,
            cache_read: 3,
            cache_creation: 4,
        };
        assert_eq!(t.total(), 10);
        let mut sum = Tokens::default();
        sum.add(&t);
        sum.add(&t);
        assert_eq!(sum.total(), 20);
        assert!(Tokens::default().is_zero());
    }
}
