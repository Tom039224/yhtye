//! Turns the parsed files into the answer: the records of the period, cut into
//! days of the client's time zone and summed per harness, model and project.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::record::{Harness, ParsedFile, Tokens};
use super::time::{format_date, local_day, local_day_start_ms};
use super::types::{
    StatsBucket, StatsDay, StatsDayHarness, StatsProject, StatsSource, StatsTotals, UsageStats,
};

/// Entries kept in `by_model` and `by_project` before the rest is summed into `other`.
pub const MAX_LISTED: usize = 20;
/// The label of work whose directory is not known.
const UNKNOWN_DIR: &str = "unknown";
const OTHER: &str = "other";

/// The period asked for: `days` local days ending with the one `now_ms` is in.
#[derive(Debug, Clone, Copy)]
pub struct Period {
    pub days: u32,
    pub tz_offset_min: i32,
    pub now_ms: u64,
}

impl Period {
    fn last_day(&self) -> i64 {
        local_day(
            i64::try_from(self.now_ms).unwrap_or(i64::MAX),
            self.tz_offset_min,
        )
    }

    fn first_day(&self) -> i64 {
        self.last_day() - (i64::from(self.days) - 1)
    }

    /// The first instant of the period (ms since the Unix epoch).
    #[must_use]
    pub fn start_ms(&self) -> i64 {
        local_day_start_ms(self.first_day(), self.tz_offset_min)
    }

    /// The first instant after the period.
    #[must_use]
    pub fn end_ms(&self) -> i64 {
        local_day_start_ms(self.last_day() + 1, self.tz_offset_min)
    }
}

/// A registered project with the directories whose sessions belong to it.
#[derive(Debug, Clone)]
pub struct ProjectRoots {
    pub id: String,
    pub name: String,
    /// The repository and the directory of its Yhtye worktrees.
    pub roots: Vec<PathBuf>,
}

/// Which project a working directory belongs to: the registered one with the
/// deepest root containing it, else the directory's own name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ProjectKey {
    Registered(usize),
    Directory(String),
}

fn project_of(cwd: &str, projects: &[ProjectRoots]) -> ProjectKey {
    let dir = Path::new(cwd);
    let deepest = projects
        .iter()
        .enumerate()
        .flat_map(|(i, p)| p.roots.iter().map(move |r| (i, r)))
        .filter(|(_, root)| !cwd.is_empty() && dir.starts_with(root))
        .max_by_key(|(_, root)| root.components().count());
    if let Some((i, _)) = deepest {
        return ProjectKey::Registered(i);
    }
    let label = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .or_else(|| (!cwd.is_empty()).then(|| cwd.to_string()))
        .unwrap_or_else(|| UNKNOWN_DIR.to_string());
    ProjectKey::Directory(label)
}

/// Tokens and the sessions that produced them.
#[derive(Default)]
struct Acc {
    tokens: Tokens,
    sessions: HashSet<u32>,
}

impl Acc {
    fn add(&mut self, tokens: &Tokens, session: u32) {
        self.tokens.add(tokens);
        self.sessions.insert(session);
    }

    fn merge(&mut self, other: Acc) {
        self.tokens.add(&other.tokens);
        self.sessions.extend(other.sessions);
    }

    fn sessions(&self) -> u64 {
        self.sessions.len() as u64
    }
}

#[derive(Default)]
struct DayAcc {
    all: Acc,
    harnesses: BTreeMap<Harness, Acc>,
}

/// One response of the period, after the cross-file de-duplication.
struct Flat {
    harness: Harness,
    session: u32,
    ts_ms: u64,
    model: Arc<str>,
    tokens: Tokens,
}

/// The records of the period of every file; a response written to several
/// files (same key) is kept once, as its largest snapshot.
fn flatten(files: &[(Harness, Arc<ParsedFile>)], period: &Period) -> (Vec<Flat>, Vec<Arc<str>>) {
    let (start, end) = (period.start_ms(), period.end_ms());
    let mut flat: Vec<Flat> = Vec::new();
    let mut by_key: HashMap<(Harness, u64), usize> = HashMap::new();
    let mut session_ids: HashMap<(Harness, Arc<str>), u32> = HashMap::new();
    // The directory of a session: the one of its earliest response.
    let mut session_cwd: Vec<(u64, Arc<str>)> = Vec::new();
    for (harness, file) in files {
        for r in &file.records {
            let ts = i64::try_from(r.ts_ms).unwrap_or(i64::MAX);
            if ts < start || ts >= end {
                continue;
            }
            let key = r.key.map(|k| (*harness, k));
            if let Some(at) = key.and_then(|k| by_key.get(&k).copied()) {
                if r.tokens.output > flat[at].tokens.output {
                    flat[at].tokens = r.tokens;
                }
                continue;
            }
            let id = (*harness, Arc::clone(&file.sessions[r.session as usize]));
            let next = u32::try_from(session_ids.len()).unwrap_or(u32::MAX);
            let session = *session_ids.entry(id).or_insert(next);
            let cwd = &file.cwds[r.cwd as usize];
            match session_cwd.get_mut(session as usize) {
                Some(earliest) if earliest.0 > r.ts_ms => *earliest = (r.ts_ms, Arc::clone(cwd)),
                Some(_) => {}
                None => session_cwd.push((r.ts_ms, Arc::clone(cwd))),
            }
            if let Some(k) = key {
                by_key.insert(k, flat.len());
            }
            flat.push(Flat {
                harness: *harness,
                session,
                ts_ms: r.ts_ms,
                model: Arc::clone(&file.models[r.model as usize]),
                tokens: r.tokens,
            });
        }
    }
    (flat, session_cwd.into_iter().map(|(_, cwd)| cwd).collect())
}

/// Sorted most tokens first (ties by name), cut to [`MAX_LISTED`] with the rest
/// summed into one `other` entry (`build` gets `None` for it).
fn ranked<K, T>(
    accs: HashMap<K, Acc>,
    name: impl Fn(&K) -> String,
    build: impl Fn(Option<&K>, String, Acc) -> T,
) -> Vec<T> {
    let mut items: Vec<(String, K, Acc)> =
        accs.into_iter().map(|(k, a)| (name(&k), k, a)).collect();
    items.sort_by(|a, b| {
        b.2.tokens
            .total()
            .cmp(&a.2.tokens.total())
            .then_with(|| a.0.cmp(&b.0))
    });
    let rest = items.split_off(items.len().min(MAX_LISTED));
    let mut out: Vec<T> = items
        .into_iter()
        .map(|(n, k, a)| build(Some(&k), n, a))
        .collect();
    if !rest.is_empty() {
        let mut other = Acc::default();
        for (_, _, a) in rest {
            other.merge(a);
        }
        out.push(build(None, OTHER.to_string(), other));
    }
    out
}

fn bucket<K>(key: Option<&K>, name: String, a: Acc) -> StatsBucket {
    StatsBucket {
        name,
        tokens: a.tokens.into(),
        sessions: a.sessions(),
        other: key.is_none(),
    }
}

/// The statistics of `files` for `period`.
#[must_use]
pub fn aggregate(
    files: &[(Harness, Arc<ParsedFile>)],
    period: &Period,
    projects: &[ProjectRoots],
    sources: Vec<StatsSource>,
) -> UsageStats {
    let (flat, session_cwd) = flatten(files, period);
    let project_of_session: Vec<ProjectKey> = session_cwd
        .iter()
        .map(|cwd| project_of(cwd, projects))
        .collect();

    let first_day = period.first_day();
    let mut days: Vec<DayAcc> = (0..period.days).map(|_| DayAcc::default()).collect();
    let mut totals = Acc::default();
    let mut models: HashMap<Arc<str>, Acc> = HashMap::new();
    let mut harnesses: HashMap<Harness, Acc> = HashMap::new();
    let mut by_project: HashMap<ProjectKey, Acc> = HashMap::new();
    for f in &flat {
        let ts = i64::try_from(f.ts_ms).unwrap_or(i64::MAX);
        let day = local_day(ts, period.tz_offset_min) - first_day;
        let Some(d) = usize::try_from(day).ok().and_then(|i| days.get_mut(i)) else {
            continue;
        };
        d.all.add(&f.tokens, f.session);
        d.harnesses
            .entry(f.harness)
            .or_default()
            .add(&f.tokens, f.session);
        totals.add(&f.tokens, f.session);
        models
            .entry(Arc::clone(&f.model))
            .or_default()
            .add(&f.tokens, f.session);
        harnesses
            .entry(f.harness)
            .or_default()
            .add(&f.tokens, f.session);
        by_project
            .entry(project_of_session[f.session as usize].clone())
            .or_default()
            .add(&f.tokens, f.session);
    }

    let days = days
        .into_iter()
        .zip(first_day..)
        .map(|(d, day)| StatsDay {
            date: format_date(day),
            tokens: d.all.tokens.into(),
            sessions: d.all.sessions(),
            harnesses: d
                .harnesses
                .into_iter()
                .map(|(h, a)| StatsDayHarness {
                    harness: h.id().to_string(),
                    tokens: a.tokens.total(),
                    sessions: a.sessions(),
                })
                .collect(),
        })
        .collect();
    UsageStats {
        generated_at_ms: period.now_ms,
        period_days: period.days,
        tz_offset_min: period.tz_offset_min,
        sources,
        totals: StatsTotals {
            tokens: totals.tokens.into(),
            sessions: totals.sessions(),
        },
        days,
        by_model: ranked(models, |m| m.to_string(), bucket),
        by_harness: ranked(harnesses, |h| h.id().to_string(), bucket),
        by_project: ranked(
            by_project,
            |k| match k {
                ProjectKey::Registered(i) => projects[*i].name.clone(),
                ProjectKey::Directory(label) => label.clone(),
            },
            |key, name, a| StatsProject {
                project: match key {
                    Some(ProjectKey::Registered(i)) => Some(projects[*i].id.clone()),
                    _ => None,
                },
                name,
                tokens: a.tokens.into(),
                sessions: a.sessions(),
                other: key.is_none(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::record::{FileBuilder, response_key};
    use crate::stats::time::parse_rfc3339_ms;

    fn at(ts: &str) -> u64 {
        parse_rfc3339_ms(ts).unwrap()
    }

    fn t(output: u64) -> Tokens {
        Tokens {
            input: 1,
            output,
            cache_read: 10,
            cache_creation: 100,
        }
    }

    /// A response row: session, model, cwd, timestamp, key, tokens.
    type Row<'a> = (&'a str, &'a str, &'a str, &'a str, Option<&'a str>, Tokens);

    fn file(rows: &[Row]) -> Arc<ParsedFile> {
        let mut b = FileBuilder::new();
        for (session, model, cwd, ts, key, tokens) in rows {
            b.push(session, model, cwd, at(ts), key.map(response_key), *tokens);
        }
        Arc::new(b.finish())
    }

    fn period(days: u32, tz_offset_min: i32) -> Period {
        Period {
            days,
            tz_offset_min,
            now_ms: at("2026-10-20T12:00:00Z"),
        }
    }

    fn roots(id: &str, name: &str, roots: &[&str]) -> ProjectRoots {
        ProjectRoots {
            id: id.into(),
            name: name.into(),
            roots: roots.iter().map(PathBuf::from).collect(),
        }
    }

    #[test]
    fn days_are_cut_in_the_clients_time_zone_and_zero_days_are_listed() {
        let f = file(&[
            ("s1", "m", "/w", "2026-10-19T20:00:00Z", None, t(1)),
            ("s2", "m", "/w", "2026-10-20T01:00:00Z", None, t(2)),
            // Before the period (7 days ending the 20th): not counted.
            ("s3", "m", "/w", "2026-10-10T01:00:00Z", None, t(4)),
        ]);
        let files = [(Harness::ClaudeCode, f)];

        let utc = aggregate(&files, &period(7, 0), &[], vec![]);
        assert_eq!(utc.days.len(), 7);
        assert_eq!(utc.days[0].date, "2026-10-14");
        assert_eq!(utc.days[6].date, "2026-10-20");
        assert_eq!(utc.days[5].tokens.output, 1);
        assert_eq!(utc.days[6].tokens.output, 2);
        assert_eq!(utc.days[3].tokens.total, 0);
        assert!(utc.days[3].harnesses.is_empty());
        assert_eq!(utc.totals.tokens.output, 3);
        assert_eq!(utc.totals.tokens.total, 225);
        assert_eq!(utc.totals.sessions, 2);

        // In Tokyo (+9) the first response is already on the 20th.
        let tokyo = aggregate(&files, &period(7, 540), &[], vec![]);
        assert_eq!(tokyo.days[5].tokens.output, 0);
        assert_eq!(tokyo.days[6].tokens.output, 3);
        assert_eq!(tokyo.days[6].sessions, 2);
        // The window starts at the local midnight: 06-10-14 00:00 +09:00.
        assert_eq!(tokyo.days[0].date, "2026-10-14");
        let edge = file(&[
            ("e1", "m", "/w", "2026-10-13T14:59:59Z", None, t(1)),
            ("e2", "m", "/w", "2026-10-13T15:00:00Z", None, t(2)),
        ]);
        let edge = aggregate(&[(Harness::Codex, edge)], &period(7, 540), &[], vec![]);
        assert_eq!(edge.totals.tokens.output, 2);
        assert_eq!(edge.days[0].harnesses[0].harness, "codex");
    }

    #[test]
    fn a_response_in_two_files_counts_once_with_its_larger_snapshot() {
        let a = file(&[("s1", "m", "/w", "2026-10-20T01:00:00Z", Some("msg_1"), t(5))]);
        let b = file(&[
            (
                "s1-resumed",
                "m",
                "/w",
                "2026-10-20T01:00:00Z",
                Some("msg_1"),
                t(9),
            ),
            (
                "s1-resumed",
                "m",
                "/w",
                "2026-10-20T02:00:00Z",
                Some("msg_2"),
                t(3),
            ),
        ]);
        let stats = aggregate(
            &[(Harness::ClaudeCode, a), (Harness::ClaudeCode, b)],
            &period(7, 0),
            &[],
            vec![],
        );
        assert_eq!(stats.totals.tokens.output, 9 + 3);
        // The duplicate belongs to the session that was seen first.
        assert_eq!(stats.totals.sessions, 2);
    }

    #[test]
    fn sessions_are_counted_per_bucket_even_when_they_span_models() {
        let f = file(&[
            ("s1", "opus", "/w", "2026-10-20T01:00:00Z", None, t(1)),
            ("s1", "haiku", "/w", "2026-10-20T02:00:00Z", None, t(1)),
            ("s2", "opus", "/w", "2026-10-20T03:00:00Z", None, t(1)),
        ]);
        let stats = aggregate(&[(Harness::ClaudeCode, f)], &period(7, 0), &[], vec![]);
        assert_eq!(stats.totals.sessions, 2);
        let by: Vec<(&str, u64, u64)> = stats
            .by_model
            .iter()
            .map(|b| (b.name.as_str(), b.tokens.output, b.sessions))
            .collect();
        assert_eq!(by, [("opus", 2, 2), ("haiku", 1, 1)]);
        assert_eq!(stats.by_harness[0].name, "claude-code");
        assert_eq!(stats.by_harness[0].sessions, 2);
    }

    #[test]
    fn sessions_belong_to_the_project_their_directory_is_in() {
        let projects = [
            roots("app", "app", &["/home/u/app", "/data/worktrees/app"]),
            roots("inner", "inner", &["/home/u/app/vendor/inner"]),
        ];
        let rows: Vec<_> = [
            ("a", "/home/u/app"),
            ("b", "/home/u/app/src"),
            ("c", "/data/worktrees/app/branches/feat/x"),
            ("d", "/home/u/app/vendor/inner/lib"),
            ("e", "/home/u/application"),
            ("f", "/tmp/scratch/one"),
            ("g", "/tmp/other/one"),
            ("h", ""),
        ]
        .into_iter()
        .map(|(s, cwd)| (s, "m", cwd, "2026-10-20T01:00:00Z", None, t(1)))
        .collect();
        let stats = aggregate(
            &[(Harness::ClaudeCode, file(&rows))],
            &period(7, 0),
            &projects,
            vec![],
        );
        let by: Vec<(&str, Option<&str>, u64)> = stats
            .by_project
            .iter()
            .map(|p| (p.name.as_str(), p.project.as_deref(), p.sessions))
            .collect();
        // "application" is not inside "app"; the two `one` directories share a label;
        // only registered projects carry their id.
        assert_eq!(
            by,
            [
                ("app", Some("app"), 3),
                ("one", None, 2),
                ("application", None, 1),
                ("inner", Some("inner"), 1),
                ("unknown", None, 1)
            ]
        );
    }

    #[test]
    fn long_lists_are_cut_with_the_rest_summed_into_other() {
        let rows: Vec<_> = (0..25_u64)
            .map(|i| {
                let model: &'static str = Box::leak(format!("model-{i:02}").into_boxed_str());
                let session: &'static str = Box::leak(format!("s{i}").into_boxed_str());
                (
                    session,
                    model,
                    "/w",
                    "2026-10-20T01:00:00Z",
                    None,
                    t(100 - i),
                )
            })
            .collect();
        let stats = aggregate(
            &[(Harness::ClaudeCode, file(&rows))],
            &period(7, 0),
            &[],
            vec![],
        );
        assert_eq!(stats.by_model.len(), MAX_LISTED + 1);
        assert_eq!(stats.by_model[0].name, "model-00");
        let other = stats.by_model.last().unwrap();
        assert!(other.other && other.name == "other");
        assert_eq!(other.sessions, 5);
        assert_eq!(
            other.tokens.output,
            (20..25_u64).map(|i| 100 - i).sum::<u64>()
        );
        assert!(stats.by_model[..MAX_LISTED].iter().all(|b| !b.other));
    }
}
