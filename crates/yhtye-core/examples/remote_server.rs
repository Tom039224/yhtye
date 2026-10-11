//! Runs a core with remote access on (the real Tailscale) without the app, for
//! trying a client (the Android app, a script) against it:
//!
//! ```text
//! cargo run -p yhtye-core --example remote_server -- [--data-dir DIR] [--port N]
//! ```
//!
//! Commands on stdin (the local-only API): `code` (a new pairing code),
//! `devices`, `revoke <device id>`, `status`, `quit`. The data directory
//! defaults to a temporary one (the app's database is not touched).

use std::io::BufRead;
use std::path::PathBuf;

use yhtye_core::api::{ApiCommand, ApiResponse};
use yhtye_core::remote::DEFAULT_PORT;
use yhtye_core::runtime::{Core, CoreConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut data_dir: Option<PathBuf> = None;
    let mut port = DEFAULT_PORT;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--data-dir" => data_dir = args.next().map(PathBuf::from),
            "--port" => port = args.next().ok_or("--port needs a value")?.parse()?,
            other => return Err(format!("unknown argument {other}").into()),
        }
    }
    let tmp = tempfile::tempdir()?;
    let dir = data_dir.unwrap_or_else(|| tmp.path().to_path_buf());
    let mut cfg = CoreConfig::claude_code(&dir, "haiku");
    cfg.usage = None;
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
            ["quit"] => break,
            [] => continue,
            _ => {
                eprintln!("commands: code | devices | status | revoke <id> | quit");
                continue;
            }
        };
        print(core.command(cmd).await);
    }
    core.shutdown().await;
    Ok(())
}

fn print(r: Result<ApiResponse, yhtye_core::api::ApiError>) {
    match r {
        Ok(ok) => println!("{}", serde_json::to_string(&ok).unwrap_or_default()),
        Err(e) => println!("error: {e}"),
    }
}
