//! TypeScript types for the UI, generated with ts-rs into `src/api/generated/`.
//!
//! `generated_typescript_is_up_to_date` fails when the committed files differ
//! from what the Rust types produce. Regenerate with `pnpm gen:types`
//! (`YHTYE_UPDATE_TS=1 cargo test -p yhtye-core --lib api::typegen`).
//!
//! Choices: `u64` is emitted as `number` (sequence numbers and millisecond
//! timestamps stay far below 2^53); ACP schema payloads inside
//! [`crate::acp::AgentEvent`] are `unknown` (the UI narrows them at runtime,
//! `src/api/acp.ts`), so the ACP crate's types are not mirrored by hand.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ts_rs::{Config, TS};

use super::{ApiCommand, ApiError, ApiEvent, ApiResponse, WsRequest, WsServerMessage};

const HEADER: &str = "// Generated from yhtye-core (ts-rs). Do not edit; run `pnpm gen:types`.\n";

fn out_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/api/generated")
}

/// Every generated file (name → contents), plus an `index.ts` re-exporting all.
fn generate() -> BTreeMap<String, String> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = Config::new()
        .with_large_int("number")
        .with_out_dir(tmp.path());
    ApiCommand::export_all(&cfg).expect("export ApiCommand");
    ApiResponse::export_all(&cfg).expect("export ApiResponse");
    ApiError::export_all(&cfg).expect("export ApiError");
    ApiEvent::export_all(&cfg).expect("export ApiEvent");
    WsRequest::export_all(&cfg).expect("export WsRequest");
    WsServerMessage::export_all(&cfg).expect("export WsServerMessage");
    let mut files = read_ts(tmp.path());
    let index: String = files
        .keys()
        .map(|name| {
            format!(
                "export type * from \"./{}\";\n",
                name.trim_end_matches(".ts")
            )
        })
        .collect();
    files.insert("index.ts".into(), format!("{HEADER}{index}"));
    files
}

/// Every `.ts` file under `dir`, by path relative to `dir` (`/`-separated).
fn read_ts(dir: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    collect_ts(dir, "", &mut files);
    files
}

fn collect_ts(dir: &Path, prefix: &str, files: &mut BTreeMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.map(|e| e.expect("dir entry").path()) {
        let name = path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        if path.is_dir() {
            collect_ts(&path, &format!("{prefix}{name}/"), files);
        } else if path.extension().is_some_and(|x| x == "ts") {
            let text = std::fs::read_to_string(&path).expect("read generated file");
            files.insert(format!("{prefix}{name}"), text);
        }
    }
}

fn write_all(dir: &Path, files: &BTreeMap<String, String>) {
    if dir.exists() {
        std::fs::remove_dir_all(dir).expect("remove the old generated files");
    }
    for (name, text) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(path, text).expect("write generated file");
    }
}

#[test]
fn generated_typescript_is_up_to_date() {
    let fresh = generate();
    let dir = out_dir();
    if std::env::var_os("YHTYE_UPDATE_TS").is_some() {
        write_all(&dir, &fresh);
        return;
    }
    let current = read_ts(&dir);
    let stale: Vec<&String> = fresh
        .keys()
        .chain(current.keys())
        .filter(|name| fresh.get(*name) != current.get(*name))
        .collect();
    assert!(
        stale.is_empty(),
        "src/api/generated is out of date ({stale:?}); run `pnpm gen:types`"
    );
}
