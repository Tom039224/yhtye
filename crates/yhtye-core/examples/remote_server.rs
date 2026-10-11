//! Runs a core with remote access on (the real Tailscale) without the app, for
//! trying a client (the Android app, a script) against it:
//!
//! ```text
//! cargo run -p yhtye-core --example remote_server -- [--data-dir DIR] [--port N]
//! ```
//!
//! Commands on stdin (the local-only API): `code` (a new pairing code),
//! `devices`, `revoke <device id>`, `status`, `quit`; and `open <path>` (opens a
//! git repository as a project, so that a client's project list is not empty).
//! The data directory defaults to a temporary one (the app's database is not
//! touched).
//!
//! `--fake-agent <yhtye-fake-agent binary>` runs every role with the fake ACP
//! agent instead of Claude Code (the Android sync layer's integration test,
//! T-66), following the scripts of `--fake-orchestrator`, `--fake-implementer`
//! and `--fake-reviewer` (`YHTYE_FAKE_SCRIPT` JSON; each defaults to doing
//! nothing).

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::Duration;

use yhtye_core::acp::{HarnessConfig, ModelSelect};
use yhtye_core::agents::{AgentChoice, HarnessPreset};
use yhtye_core::api::{ApiCommand, ApiResponse};
use yhtye_core::remote::DEFAULT_PORT;
use yhtye_core::runtime::{Core, CoreConfig};

/// The fake agent's script that answers nothing (`--fake-*` not given).
const IDLE_SCRIPT: &str = r#"{"turns": []}"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut data_dir: Option<PathBuf> = None;
    let mut port = DEFAULT_PORT;
    let mut fake_agent: Option<PathBuf> = None;
    let [mut orchestrator, mut implementer, mut reviewer] = [
        IDLE_SCRIPT.to_string(),
        IDLE_SCRIPT.into(),
        IDLE_SCRIPT.into(),
    ];
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            "--port" => port = value()?.parse()?,
            "--fake-agent" => fake_agent = Some(PathBuf::from(value()?)),
            "--fake-orchestrator" => orchestrator = value()?,
            "--fake-implementer" => implementer = value()?,
            "--fake-reviewer" => reviewer = value()?,
            other => return Err(format!("unknown argument {other}").into()),
        }
    }
    let tmp = tempfile::tempdir()?;
    let dir = data_dir.unwrap_or_else(|| tmp.path().to_path_buf());
    let mut cfg = CoreConfig::claude_code(&dir, "haiku");
    cfg.usage = None;
    if let Some(bin) = fake_agent {
        cfg.harnesses = vec![HarnessPreset::fixed(
            "fake",
            fake_harness(&bin, orchestrator),
            fake_harness(&bin, implementer),
            fake_harness(&bin, reviewer),
        )];
        cfg.default_agent = AgentChoice::new("fake", None);
    }
    let core = Core::start(cfg).await?;
    let cmd = ApiCommand::SetRemoteSettings {
        enabled: true,
        port: Some(port),
    };
    print(core.command(cmd).await);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    while let Some(line) = rx.recv().await {
        let words: Vec<&str> = line.split_whitespace().collect();
        let cmd = match words.as_slice() {
            ["code"] => ApiCommand::CreatePairingCode,
            ["devices"] => ApiCommand::ListRemoteDevices,
            ["status"] => ApiCommand::GetRemoteStatus,
            ["revoke", id] => ApiCommand::RevokeRemoteDevice {
                device: (*id).to_string(),
            },
            ["open", path] => ApiCommand::OpenProject {
                path: (*path).to_string(),
            },
            ["quit"] => break,
            [] => continue,
            _ => {
                eprintln!("commands: code | devices | status | revoke <id> | open <path> | quit");
                continue;
            }
        };
        print(core.command(cmd).await);
    }
    core.shutdown().await;
    Ok(())
}

/// The fake ACP agent following `script` (as the core's own tests configure it).
fn fake_harness(bin: &Path, script: String) -> HarnessConfig {
    let mut h = HarnessConfig::plain(bin.display().to_string(), vec![]);
    h.env.insert("YHTYE_FAKE_SCRIPT".into(), script);
    h.mode_after_new = Some("bypassPermissions".into());
    h.model = Some(ModelSelect {
        config_id: "model".into(),
        value: "haiku".into(),
    });
    h.startup_timeout = Duration::from_secs(10);
    h
}

fn print(r: Result<ApiResponse, yhtye_core::api::ApiError>) {
    match r {
        Ok(ok) => println!("{}", serde_json::to_string(&ok).unwrap_or_default()),
        Err(e) => println!("error: {e}"),
    }
}
