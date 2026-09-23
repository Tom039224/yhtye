//! `yhtye-dev-bridge`: runs the Yhtye core (Claude Code for every role) and
//! serves its API on `ws://127.0.0.1:<port>/ws` for the React app in a browser.
//! See `yhtye_dev_bridge` and `core-design.md` §10.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;
use yhtye_core::runtime::{Core, CoreConfig};
use yhtye_dev_bridge::{BridgeConfig, DEFAULT_PORT, WS_PATH, random_token, serve};

const USAGE: &str = "\
usage: yhtye-dev-bridge [--data-dir DIR] [--port N] [--model NAME] [--token T] [--allow-origin URL]...

  --data-dir DIR       database and worktrees (default: $YHTYE_DATA_DIR, else
                       $XDG_DATA_HOME/yhtye-dev-bridge, else ~/.local/share/yhtye-dev-bridge)
  --port N             127.0.0.1 port (default: 1422)
  --model NAME         Claude Code model for every role (default: $YHTYE_MODEL, else haiku)
  --token T            required ?token= (default: $YHTYE_BRIDGE_TOKEN, else random)
  --allow-origin URL   extra allowed browser origin (http://localhost:1420 and
                       http://127.0.0.1:1420 are always allowed)
";

struct Args {
    data_dir: PathBuf,
    port: u16,
    model: String,
    token: Option<String>,
    origins: Vec<String>,
}

fn default_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("YHTYE_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("yhtye-dev-bridge")
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        data_dir: default_data_dir(),
        port: DEFAULT_PORT,
        model: std::env::var("YHTYE_MODEL").unwrap_or_else(|_| "haiku".into()),
        token: std::env::var("YHTYE_BRIDGE_TOKEN")
            .ok()
            .filter(|t| !t.is_empty()),
        origins: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--data-dir" => args.data_dir = PathBuf::from(value()?),
            "--port" => {
                let v = value()?;
                args.port = v.parse().map_err(|_| format!("bad port {v}"))?;
            }
            "--model" => args.model = value()?,
            "--token" => args.token = Some(value()?),
            "--allow-origin" => args.origins.push(value()?),
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,yhtye_core=info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("{e}");
            }
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("yhtye-dev-bridge: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), String> {
    let token = match args.token {
        Some(t) => t,
        None => random_token().map_err(|e| format!("no random token: {e}"))?,
    };
    let mut cfg = BridgeConfig::new(token.clone());
    cfg.allowed_origins.extend(args.origins);
    std::fs::create_dir_all(&args.data_dir)
        .map_err(|e| format!("{}: {e}", args.data_dir.display()))?;
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("cannot listen on {addr}: {e}"))?;
    let core = Core::start(CoreConfig::installed(&args.data_dir, &args.model))
        .await
        .map_err(|e| format!("cannot start the core: {e}"))?;
    tracing::info!(
        "data dir {}, model {}; listening on ws://{addr}{WS_PATH}",
        args.data_dir.display(),
        args.model
    );
    // Machine-readable line for scripts (the token is a local dev secret).
    println!("YHTYE_BRIDGE_URL=ws://{addr}{WS_PATH}?token={token}");

    // Interrupted work resumes without waiting for a client to open the project.
    let resume = core.clone();
    tokio::spawn(async move {
        resume.resume_unfinished().await;
    });
    let served = serve(listener, core.clone(), cfg, shutdown_signal()).await;
    tracing::info!("stopping: shutting down every agent");
    core.shutdown().await;
    tracing::info!("stopped");
    served.map_err(|e| format!("server error: {e}"))
}

/// SIGINT or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("cannot listen for SIGTERM: {e}");
                    let _ = ctrl_c.await;
                    return;
                }
            };
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}
