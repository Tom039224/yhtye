//! The version `ping` reports (`APP_VERSION`, the workspace version) is the
//! version of the desktop app everywhere it is written down: the Android client
//! checks it against its own (see docs/DEVELOPMENT.md, "Releasing the Android app").

use std::path::PathBuf;

use serde_json::Value;
use yhtye_core::runtime::APP_VERSION;

fn repo_file(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

fn json_version(path: &str) -> String {
    let json: Value = serde_json::from_str(&repo_file(path)).expect("json");
    json["version"].as_str().expect("version").to_string()
}

#[test]
fn the_app_version_is_the_same_in_every_manifest() {
    assert_eq!(json_version("src-tauri/tauri.conf.json"), APP_VERSION);
    assert_eq!(json_version("package.json"), APP_VERSION);
}

#[test]
fn the_tauri_shell_takes_the_workspace_version() {
    let manifest = repo_file("src-tauri/Cargo.toml");
    assert!(
        manifest
            .lines()
            .any(|l| l.trim() == "version.workspace = true"),
        "src-tauri/Cargo.toml must use `version.workspace = true`"
    );
}
