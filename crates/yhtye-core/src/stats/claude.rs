//! Claude Code: `<projects>/<encoded cwd>/<session>.jsonl`, plus the files of
//! its subagents (`<session>/subagents/agent-*.jsonl`, lines marked
//! `isSidechain`, carrying the parent's `sessionId`).
//!
//! An assistant line has `timestamp`, `cwd`, `sessionId` and `message`
//! (`id`, `model`, `usage`). A response is written as one line per content
//! block, all with the same `message.id` and usage, so the last line of an id
//! is the response. Sessions that only ran `/usage` (Yhtye's own usage probe
//! does) are left out.

use std::borrow::Cow;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use memchr::memmem::Finder;
use serde::Deserialize;

use super::record::{FileBuilder, ParsedFile, Tokens, response_key};
use super::time::parse_rfc3339_ms;

/// The model name of the placeholder responses Claude Code writes for errors
/// and interruptions (no tokens were used).
const SYNTHETIC_MODEL: &str = "<synthetic>";
/// Read buffer: the files are large and read once, front to back.
const READ_BUFFER: usize = 256 * 1024;
/// A line buffer that grew past this (a huge tool result) is released.
const KEEP_LINE_BUFFER: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
struct AssistantLine<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
    #[serde(rename = "sessionId", borrow)]
    session_id: Option<Cow<'a, str>>,
    message: Option<AssistantMessage<'a>>,
}

#[derive(Deserialize)]
struct AssistantMessage<'a> {
    #[serde(borrow)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct UserLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "isMeta")]
    is_meta: Option<bool>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    message: Option<UserMessage>,
}

#[derive(Deserialize)]
struct UserMessage {
    content: Option<Content>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Blocks(Vec<Block>),
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

/// What a user turn is, for telling probe sessions from real ones.
#[derive(Debug, PartialEq, Eq)]
enum Turn {
    /// The `/usage` command.
    UsageCommand,
    /// Something the user wrote.
    Prompt,
    /// Bookkeeping of the CLI (`<local-command-…>` echoes, empty text).
    Other,
}

fn classify(text: &str) -> Turn {
    let t = text.trim();
    if t == "/usage" || t.contains("<command-name>/usage</command-name>") {
        Turn::UsageCommand
    } else if t.is_empty() || t.starts_with("<local-command-") {
        Turn::Other
    } else {
        Turn::Prompt
    }
}

fn user_turn(line: &UserLine) -> Turn {
    if line.kind.as_deref() != Some("user")
        || line.is_meta == Some(true)
        || line.is_sidechain == Some(true)
    {
        return Turn::Other;
    }
    match line.message.as_ref().and_then(|m| m.content.as_ref()) {
        Some(Content::Text(text)) => classify(text),
        Some(Content::Blocks(blocks)) => {
            let mut turn = Turn::Other;
            for text in blocks
                .iter()
                .filter(|b| b.kind.as_deref() == Some("text"))
                .filter_map(|b| b.text.as_deref())
            {
                match classify(text) {
                    Turn::Prompt => return Turn::Prompt,
                    Turn::UsageCommand => turn = Turn::UsageCommand,
                    Turn::Other => {}
                }
            }
            turn
        }
        None => Turn::Other,
    }
}

/// The session a file without `sessionId` lines belongs to: the file name,
/// or for a subagent file (`<session>/subagents/agent-*.jsonl`) the directory
/// of its session.
#[must_use]
pub fn fallback_session(path: &Path) -> String {
    let parent = path.parent();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let in_subagents = parent
        .and_then(Path::file_name)
        .is_some_and(|n| n == "subagents");
    if in_subagents && let Some(session) = parent.and_then(Path::parent).and_then(Path::file_name) {
        return session.to_string_lossy().into_owned();
    }
    stem
}

/// Reads the file at `path`.
pub fn parse_file(path: &Path) -> io::Result<ParsedFile> {
    let file = std::fs::File::open(path)?;
    parse(
        BufReader::with_capacity(READ_BUFFER, file),
        &fallback_session(path),
    )
}

/// Reads Claude Code JSONL. Lines that cannot be read are skipped. A reader
/// error ends the parse and is returned.
pub fn parse<R: BufRead>(mut reader: R, fallback_session: &str) -> io::Result<ParsedFile> {
    let assistant = Finder::new(br#""type":"assistant""#);
    let user = Finder::new(br#""type":"user""#);
    let queue = Finder::new(br#""type":"queue-operation""#);
    let tool_result = Finder::new(br#""tool_use_id""#);
    let queued_usage = Finder::new(br#""content":"/usage""#);

    let mut builder = FileBuilder::new();
    let mut prompts = 0_u32;
    let mut usage_commands = 0_u32;
    let mut line = Vec::new();
    loop {
        line.clear();
        if line.capacity() > KEEP_LINE_BUFFER {
            line = Vec::new();
        }
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if assistant.find(&line).is_some() {
            add_assistant(&mut builder, &line, fallback_session);
        } else if user.find(&line).is_some() && tool_result.find(&line).is_none() {
            match serde_json::from_slice::<UserLine>(&line).map(|l| user_turn(&l)) {
                Ok(Turn::Prompt) => prompts += 1,
                Ok(Turn::UsageCommand) => usage_commands += 1,
                Ok(Turn::Other) | Err(_) => {}
            }
        } else if queue.find(&line).is_some() && queued_usage.find(&line).is_some() {
            usage_commands += 1;
        }
    }
    if usage_commands > 0 && prompts == 0 {
        return Ok(ParsedFile::default());
    }
    Ok(builder.finish())
}

fn add_assistant(builder: &mut FileBuilder, line: &[u8], fallback_session: &str) {
    let Ok(l) = serde_json::from_slice::<AssistantLine>(line) else {
        return;
    };
    if l.kind.as_deref() != Some("assistant") {
        return;
    }
    let (Some(message), Some(ts_ms)) =
        (l.message, l.timestamp.as_deref().and_then(parse_rfc3339_ms))
    else {
        return;
    };
    let Some(usage) = message.usage else {
        return;
    };
    let model = message.model.as_deref().unwrap_or_default();
    if model == SYNTHETIC_MODEL {
        return;
    }
    let tokens = Tokens {
        input: usage.input_tokens.unwrap_or(0),
        output: usage.output_tokens.unwrap_or(0),
        cache_read: usage.cache_read_input_tokens.unwrap_or(0),
        cache_creation: usage.cache_creation_input_tokens.unwrap_or(0),
    };
    if tokens.is_zero() {
        return;
    }
    builder.push(
        l.session_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback_session),
        model,
        l.cwd.as_deref().unwrap_or_default(),
        ts_ms,
        message.id.as_deref().map(response_key),
        tokens,
    );
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::json;

    use super::*;

    fn jsonl(lines: &[serde_json::Value]) -> Cursor<Vec<u8>> {
        let mut out = String::new();
        for l in lines {
            out.push_str(&l.to_string());
            out.push('\n');
        }
        Cursor::new(out.into_bytes())
    }

    fn assistant(id: &str, ts: &str, output: u64) -> serde_json::Value {
        json!({
            "type": "assistant", "timestamp": ts, "cwd": "/work/app", "sessionId": "s1",
            "isSidechain": false,
            "message": {
                "id": id, "model": "claude-opus-5", "role": "assistant",
                "content": [{"type": "text", "text": "hi \"there\""}],
                "usage": {
                    "input_tokens": 3, "output_tokens": output,
                    "cache_creation_input_tokens": 100, "cache_read_input_tokens": 1000
                }
            }
        })
    }

    #[test]
    fn a_response_written_several_times_counts_once_with_its_last_usage() {
        let f = parse(
            jsonl(&[
                assistant("msg_a", "2026-10-01T10:00:00.000Z", 5),
                assistant("msg_a", "2026-10-01T10:00:01.000Z", 5),
                assistant("msg_a", "2026-10-01T10:00:02.000Z", 40),
                assistant("msg_b", "2026-10-01T10:00:05.000Z", 7),
            ]),
            "fallback",
        )
        .unwrap();
        assert_eq!(f.records.len(), 2);
        let a = &f.records[0];
        assert_eq!(
            (
                a.tokens.input,
                a.tokens.output,
                a.tokens.cache_read,
                a.tokens.cache_creation
            ),
            (3, 40, 1000, 100)
        );
        assert_eq!(&*f.models[a.model as usize], "claude-opus-5");
        assert_eq!(&*f.cwds[a.cwd as usize], "/work/app");
        assert_eq!(&*f.sessions[a.session as usize], "s1");
        assert_eq!(f.records[1].tokens.output, 7);
    }

    #[test]
    fn sidechain_lines_count_toward_the_session_they_carry() {
        let mut side = assistant("msg_side", "2026-10-01T10:01:00.000Z", 9);
        side["isSidechain"] = json!(true);
        let f = parse(
            jsonl(&[assistant("msg_a", "2026-10-01T10:00:00.000Z", 5), side]),
            "fallback",
        )
        .unwrap();
        assert_eq!(f.records.len(), 2);
        assert_eq!(f.sessions.len(), 1);
    }

    #[test]
    fn lines_without_usable_data_are_skipped() {
        let mut no_usage = assistant("msg_n", "2026-10-01T10:00:00.000Z", 1);
        no_usage["message"].as_object_mut().unwrap().remove("usage");
        let mut synthetic = assistant("msg_s", "2026-10-01T10:00:00.000Z", 1);
        synthetic["message"]["model"] = json!("<synthetic>");
        let mut zero = assistant("msg_z", "2026-10-01T10:00:00.000Z", 0);
        zero["message"]["usage"] = json!({"input_tokens": 0, "output_tokens": 0});
        let no_time = assistant("msg_t", "yesterday", 1);
        let mut data = jsonl(&[no_usage, synthetic, zero, no_time]).into_inner();
        data.extend_from_slice(b"{\"type\":\"assistant\" truncated\n");
        data.extend_from_slice(b"not json at all\n");
        data.extend_from_slice(
            assistant("msg_ok", "2026-10-01T10:00:00.000Z", 2)
                .to_string()
                .as_bytes(),
        );
        let f = parse(Cursor::new(data), "fallback").unwrap();
        assert_eq!(f.records.len(), 1);
        assert_eq!(f.records[0].tokens.output, 2);
    }

    #[test]
    fn a_session_id_missing_from_the_lines_falls_back_to_the_file() {
        let mut line = assistant("msg_a", "2026-10-01T10:00:00.000Z", 5);
        line.as_object_mut().unwrap().remove("sessionId");
        let f = parse(jsonl(&[line]), "from-file").unwrap();
        assert_eq!(&*f.sessions[0], "from-file");
        assert_eq!(
            fallback_session(Path::new("/p/-proj/abc.jsonl")),
            "abc".to_string()
        );
        assert_eq!(
            fallback_session(Path::new("/p/-proj/sess-1/subagents/agent-x.jsonl")),
            "sess-1".to_string()
        );
    }

    fn user(content: serde_json::Value) -> serde_json::Value {
        json!({"type": "user", "isSidechain": false, "message": {"role": "user", "content": content}})
    }

    #[test]
    fn a_session_that_only_ran_usage_is_a_probe_and_counts_nothing() {
        let probe = || {
            vec![
                user(json!(
                    "<local-command-caveat>Caveat: ...</local-command-caveat>"
                )),
                user(json!(
                    "<command-name>/usage</command-name>\n<command-message>usage</command-message>"
                )),
                assistant("msg_a", "2026-10-01T10:00:00.000Z", 5),
            ]
        };
        assert!(parse(jsonl(&probe()), "f").unwrap().records.is_empty());

        // The command may also be written as a queue operation.
        let mut queued = vec![json!({
            "type": "queue-operation", "operation": "enqueue", "content": "/usage"
        })];
        queued.push(assistant("msg_a", "2026-10-01T10:00:00.000Z", 5));
        assert!(parse(jsonl(&queued), "f").unwrap().records.is_empty());

        // A real prompt (text blocks) makes it a real session, /usage or not.
        let mut real = vec![user(json!([{"type": "text", "text": "fix the bug"}]))];
        real.extend(probe());
        assert_eq!(parse(jsonl(&real), "f").unwrap().records.len(), 1);

        // Tool results are not prompts; without any command it is no probe.
        let results = vec![
            user(json!([{"type": "tool_result", "tool_use_id": "t1", "content": "/usage"}])),
            assistant("msg_a", "2026-10-01T10:00:00.000Z", 5),
        ];
        assert_eq!(parse(jsonl(&results), "f").unwrap().records.len(), 1);
    }
}
