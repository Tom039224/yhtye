//! Codex: rollout files `rollout-*.jsonl` below `sessions/` and
//! `archived_sessions/`.
//!
//! `session_meta` names the session and its `cwd`; a rollout with
//! `forked_from_id` replays its parent's lines (the parent's token counts
//! too), so it is skipped to count them once. `turn_context` carries the model.
//! `event_msg` lines of type `token_count` carry `info.last_token_usage` (the
//! response just finished) and `info.total_token_usage` (the running sum):
//! Codex repeats an event (rate-limit updates), so one whose running sum did
//! not change counts nothing.

use std::io::{self, BufRead, BufReader};
use std::path::Path;

use memchr::memmem::Finder;
use serde::Deserialize;
use serde_json::Value;

use super::record::{FileBuilder, ParsedFile, Tokens};
use super::time::parse_rfc3339_ms;

const READ_BUFFER: usize = 64 * 1024;
const UNKNOWN_MODEL: &str = "unknown";

#[derive(Deserialize)]
struct Line {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    payload: Option<Value>,
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn count(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Codex's `input_tokens` is the whole input, cached part included: the
/// uncached remainder is the input, the cached part the cache read, and a cache
/// write (inside the input as well) is split out of it.
fn normalize(usage: &Value) -> Tokens {
    let total_input = count(usage, "input_tokens");
    let cached = count(usage, "cached_input_tokens");
    let uncached = total_input.saturating_sub(cached);
    let cache_creation = count(usage, "cache_write_input_tokens").min(uncached);
    Tokens {
        input: uncached - cache_creation,
        output: count(usage, "output_tokens"),
        cache_read: cached,
        cache_creation,
    }
}

/// Reads the rollout at `path`.
pub fn parse_file(path: &Path) -> io::Result<ParsedFile> {
    let file = std::fs::File::open(path)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    parse(BufReader::with_capacity(READ_BUFFER, file), &stem)
}

/// Reads a Codex rollout. Lines that cannot be read are skipped; a reader
/// error ends the parse and is returned.
pub fn parse<R: BufRead>(mut reader: R, fallback_session: &str) -> io::Result<ParsedFile> {
    let session_meta = Finder::new(br#""type":"session_meta""#);
    let turn_context = Finder::new(br#""type":"turn_context""#);
    let token_count = Finder::new(br#""type":"token_count""#);

    let mut builder = FileBuilder::new();
    let mut session: Option<String> = None;
    let mut cwd = String::new();
    let mut model = String::from(UNKNOWN_MODEL);
    let mut last_total: Option<Value> = None;
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        let wanted = session_meta.find(&line).is_some()
            || turn_context.find(&line).is_some()
            || token_count.find(&line).is_some();
        if !wanted {
            continue;
        }
        let Ok(Line {
            timestamp,
            kind: Some(kind),
            payload: Some(payload),
        }) = serde_json::from_slice::<Line>(&line)
        else {
            continue;
        };
        match kind.as_str() {
            "session_meta" => {
                if text(&payload, "forked_from_id").is_some() {
                    return Ok(ParsedFile::default());
                }
                if session.is_none() {
                    session = text(&payload, "id").map(str::to_string);
                    if let Some(dir) = text(&payload, "cwd") {
                        dir.clone_into(&mut cwd);
                    }
                }
            }
            "turn_context" => {
                if let Some(m) = text(&payload, "model") {
                    m.clone_into(&mut model);
                }
                if cwd.is_empty()
                    && let Some(dir) = text(&payload, "cwd")
                {
                    dir.clone_into(&mut cwd);
                }
            }
            "event_msg" if text(&payload, "type") == Some("token_count") => {
                let Some(info) = payload.get("info").filter(|i| i.is_object()) else {
                    continue;
                };
                let total = info.get("total_token_usage").cloned();
                if total.is_some() && total == last_total {
                    continue;
                }
                last_total = total;
                let tokens = info
                    .get("last_token_usage")
                    .map(normalize)
                    .filter(|t| !t.is_zero());
                let (Some(tokens), Some(ts_ms)) =
                    (tokens, timestamp.as_deref().and_then(parse_rfc3339_ms))
                else {
                    continue;
                };
                builder.push(
                    session.as_deref().unwrap_or(fallback_session),
                    &model,
                    &cwd,
                    ts_ms,
                    None,
                    tokens,
                );
            }
            _ => {}
        }
    }
    Ok(builder.finish())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::json;

    use super::*;

    fn jsonl(lines: &[Value]) -> Cursor<Vec<u8>> {
        let mut out = String::new();
        for l in lines {
            out.push_str(&l.to_string());
            out.push('\n');
        }
        Cursor::new(out.into_bytes())
    }

    fn meta(extra: Value) -> Value {
        let mut payload = json!({"id": "sess-1", "cwd": "/work/app", "originator": "codex_cli"});
        for (k, v) in extra.as_object().unwrap() {
            payload[k] = v.clone();
        }
        json!({"timestamp": "2026-10-01T10:00:00.000Z", "type": "session_meta", "payload": payload})
    }

    fn turn(model: &str) -> Value {
        json!({"timestamp": "2026-10-01T10:00:01.000Z", "type": "turn_context",
            "payload": {"turn_id": "t1", "cwd": "/work/app", "model": model}})
    }

    fn tokens(ts: &str, last: Value, total: Value) -> Value {
        json!({"timestamp": ts, "type": "event_msg", "payload": {
            "type": "token_count",
            "info": {"last_token_usage": last, "total_token_usage": total}}})
    }

    fn usage(input: u64, cached: u64, output: u64) -> Value {
        json!({"input_tokens": input, "cached_input_tokens": cached, "output_tokens": output,
            "total_tokens": input + output})
    }

    #[test]
    fn cached_input_is_taken_out_of_the_input() {
        let f = parse(
            jsonl(&[
                meta(json!({})),
                turn("gpt-5-codex"),
                tokens(
                    "2026-10-01T10:00:02.000Z",
                    usage(1000, 600, 50),
                    usage(1000, 600, 50),
                ),
                tokens(
                    "2026-10-01T10:00:09.000Z",
                    usage(2000, 1800, 20),
                    usage(3000, 2400, 70),
                ),
            ]),
            "fallback",
        )
        .unwrap();
        assert_eq!(f.records.len(), 2);
        let t = &f.records[0].tokens;
        assert_eq!(
            (t.input, t.cache_read, t.output, t.cache_creation),
            (400, 600, 50, 0)
        );
        let t = &f.records[1].tokens;
        assert_eq!((t.input, t.cache_read, t.output), (200, 1800, 20));
        assert_eq!(&*f.models[f.records[0].model as usize], "gpt-5-codex");
        assert_eq!(&*f.sessions[0], "sess-1");
        assert_eq!(&*f.cwds[0], "/work/app");
    }

    #[test]
    fn cache_writes_are_split_out_of_the_uncached_input() {
        let last = json!({"input_tokens": 1000, "cached_input_tokens": 100,
            "cache_write_input_tokens": 300, "output_tokens": 5});
        let f = parse(
            jsonl(&[
                meta(json!({})),
                tokens("2026-10-01T10:00:02.000Z", last.clone(), last),
            ]),
            "f",
        )
        .unwrap();
        let t = &f.records[0].tokens;
        assert_eq!((t.input, t.cache_read, t.cache_creation), (600, 100, 300));
    }

    #[test]
    fn a_repeated_event_counts_once() {
        let u = usage(100, 0, 10);
        let f = parse(
            jsonl(&[
                meta(json!({})),
                tokens("2026-10-01T10:00:02.000Z", u.clone(), u.clone()),
                tokens("2026-10-01T10:00:03.000Z", u.clone(), u),
                json!({"timestamp": "2026-10-01T10:00:04.000Z", "type": "event_msg",
                    "payload": {"type": "token_count", "info": null}}),
            ]),
            "f",
        )
        .unwrap();
        assert_eq!(f.records.len(), 1);
    }

    #[test]
    fn a_forked_rollout_is_skipped_to_avoid_counting_its_parent_twice() {
        let f = parse(
            jsonl(&[
                meta(json!({"forked_from_id": "parent-1"})),
                turn("gpt-5-codex"),
                tokens(
                    "2026-10-01T10:00:02.000Z",
                    usage(100, 0, 10),
                    usage(100, 0, 10),
                ),
            ]),
            "f",
        )
        .unwrap();
        assert!(f.records.is_empty());
    }

    #[test]
    fn the_model_follows_the_latest_turn_and_defaults_to_unknown() {
        let f = parse(
            jsonl(&[
                tokens("2026-10-01T10:00:01.000Z", usage(10, 0, 1), usage(10, 0, 1)),
                turn("gpt-5"),
                tokens("2026-10-01T10:00:02.000Z", usage(10, 0, 1), usage(20, 0, 2)),
                turn("gpt-5-mini"),
                tokens("2026-10-01T10:00:03.000Z", usage(10, 0, 1), usage(30, 0, 3)),
            ]),
            "from-file",
        )
        .unwrap();
        let models: Vec<&str> = f
            .records
            .iter()
            .map(|r| &*f.models[r.model as usize])
            .collect();
        assert_eq!(models, ["unknown", "gpt-5", "gpt-5-mini"]);
        assert_eq!(&*f.sessions[0], "from-file");
    }
}
