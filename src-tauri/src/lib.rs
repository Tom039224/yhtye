//! The Tauri shell (`core-design.md` §9): starts the [`Core`] with the
//! production configuration, forwards `yhtye_command` to it, emits every core
//! event as `yhtye://event`, starts remote access if it is on, and shuts every
//! agent down (and remote access) when the app exits.
//!
//! `main` first looks at the command line ([`cli`]): `--headless` runs the core
//! without any of Tauri ([`headless`]), `yhtye remote …` talks to such an
//! instance, and only the plain command opens the window.

pub mod cli;
pub mod data_dir;
mod data_lock;
#[cfg(unix)]
pub mod headless;
mod legacy_data;
mod webkit_env;

use std::io::IsTerminal;

use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tokio::sync::broadcast;
use yhtye_core::api::{ApiCommand, ApiError, ApiEvent, ApiEventBody, ApiResponse};
use yhtye_core::runtime::{Core, CoreConfig};

use cli::Options;
use data_dir::DataDir;

/// Without the Unix socket and the signals of Unix there is no headless mode.
#[cfg(not(unix))]
pub mod headless {
    use std::process::ExitCode;

    use crate::cli::{Options, RemoteAction};

    const UNSUPPORTED: &str = "Yhtye: headless mode is only supported on Unix";

    pub fn run(_: Options) -> ExitCode {
        eprintln!("{UNSUPPORTED}");
        ExitCode::FAILURE
    }

    pub fn run_remote(_: &RemoteAction, _: Option<std::path::PathBuf>) -> ExitCode {
        eprintln!("{UNSUPPORTED}");
        ExitCode::FAILURE
    }
}

/// Event name the frontend listens to (`src/api/tauri.ts`).
const EVENT: &str = "yhtye://event";

/// The one command: runs an [`ApiCommand`] on the core.
#[tauri::command]
async fn yhtye_command(core: State<'_, Core>, cmd: ApiCommand) -> Result<ApiResponse, ApiError> {
    core.command(cmd).await
}

/// Locks the data directory (one app per directory, see [`data_lock`]) and
/// starts the core on it. The directory is [`DataDir`]'s: `--data-dir`, else
/// `YHTYE_DATA_DIR`, else the app data directory (the pre-rename
/// `com.tom039224.yhtye` one is moved first).
fn start_core(opts: &Options) -> Result<(Core, data_lock::DataLock), Box<dyn std::error::Error>> {
    let dir = DataDir::resolve(opts.data_dir.as_deref())?;
    dir.prepare()?;
    let lock = data_lock::acquire(&dir.path)?;
    let model = data_dir::model(opts.model.as_deref());
    tracing::info!("data dir {}, model {model}", dir.path.display());
    let core =
        tauri::async_runtime::block_on(Core::start(CoreConfig::installed(&dir.path, &model)))?;
    Ok((core, lock))
}

/// Emits every core event to the webview. A lagging receiver skips events; the
/// webview is then sent a `resync` hint and catches up (the event log, the
/// project list, the settings on display; T-65).
fn forward_events(app: AppHandle, mut rx: broadcast::Receiver<ApiEvent>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("the webview fell behind; {n} events skipped");
                    ApiEvent::app_wide(ApiEventBody::Resync)
                }
                Err(broadcast::error::RecvError::Closed) => return,
            };
            if let Err(e) = app.emit(EVENT, &ev) {
                tracing::warn!("could not emit an event: {e}");
            }
        }
    });
}

/// SIGINT / SIGTERM (e.g. Ctrl+C on `pnpm tauri dev`) exit through the normal
/// path, so the agents are shut down too.
fn exit_on_signal(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let (Ok(mut int), Ok(mut term)) = (
                signal(SignalKind::interrupt()),
                signal(SignalKind::terminate()),
            ) else {
                tracing::error!("cannot listen for signals");
                return;
            };
            tokio::select! {
                _ = int.recv() => {}
                _ = term.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
        }
        tracing::info!("signal received; exiting");
        app.exit(0);
    });
}

/// Logs to stdout (the app) or stderr (`--headless`: stdout is for people).
pub(crate) fn init_tracing(headless: bool) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    let _ = if headless {
        builder
            .with_writer(std::io::stderr)
            // A service manager's log is not a terminal: no colour codes there.
            .with_ansi(std::io::stderr().is_terminal())
            .try_init()
    } else {
        builder.try_init()
    };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    run_app(&Options::default());
}

/// Opens the window. Needs a display (GTK starts in `Builder::build`).
pub fn run_app(opts: &Options) {
    webkit_env::apply();
    init_tracing(false);
    let opts = opts.clone();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![yhtye_command])
        .setup(move |app| {
            let handle = app.handle().clone();
            let (core, lock) = match start_core(&opts) {
                Ok(started) => started,
                Err(e) if e.is::<data_lock::LockError>() => {
                    // Not a crash: the other app is the one in use.
                    tracing::error!("{e}");
                    eprintln!("Yhtye: {e}");
                    std::process::exit(1);
                }
                Err(e) => return Err(e),
            };
            app.manage(lock);
            forward_events(handle.clone(), core.subscribe());
            // Interrupted work resumes without waiting for the UI to open it.
            let resume = core.clone();
            tauri::async_runtime::spawn(async move {
                resume.resume_unfinished().await;
            });
            // Remote access (Android client), if the settings turn it on; it
            // stops with `Core::shutdown` on exit.
            let remote = core.clone();
            tauri::async_runtime::spawn(async move {
                remote.start_remote().await;
            });
            app.manage(core);
            exit_on_signal(handle);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the Yhtye app");
    app.run(|app, event| {
        if let RunEvent::Exit = event {
            if let Some(core) = app.try_state::<Core>() {
                tracing::info!("exiting: shutting down every agent");
                tauri::async_runtime::block_on(core.shutdown());
                tracing::info!("all agents stopped");
            }
        }
    });
}
