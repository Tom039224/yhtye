//! The Tauri shell (`core-design.md` §9): starts the [`Core`] with the
//! production configuration, forwards `yhtye_command` to it, emits every core
//! event as `yhtye://event`, starts remote access if it is on, and shuts every
//! agent down (and remote access) when the app exits.

mod data_lock;
mod legacy_data;
mod webkit_env;

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use tokio::sync::broadcast;
use yhtye_core::api::{ApiCommand, ApiError, ApiEvent, ApiEventBody, ApiResponse};
use yhtye_core::runtime::{Core, CoreConfig};

/// Event name the frontend listens to (`src/api/tauri.ts`).
const EVENT: &str = "yhtye://event";

/// Model of every role unless `YHTYE_MODEL` says otherwise (Haiku while the
/// rebuild is being verified; docs/PLAN.md).
const DEFAULT_MODEL: &str = "haiku";

/// The one command: runs an [`ApiCommand`] on the core.
#[tauri::command]
async fn yhtye_command(core: State<'_, Core>, cmd: ApiCommand) -> Result<ApiResponse, ApiError> {
    core.command(cmd).await
}

/// `YHTYE_DATA_DIR`, else the app data directory
/// (`~/.local/share/io.github.tom039224.yhtye` on Linux): the database and worktrees.
/// On the default path the pre-rename `com.tom039224.yhtye` directory is moved first.
fn data_dir(app: &AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(dir) = std::env::var_os("YHTYE_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let dir = app.path().app_data_dir()?;
    legacy_data::migrate(&dir);
    Ok(dir)
}

/// Locks the data directory (one app per directory, see [`data_lock`]) and
/// starts the core on it.
fn start_core(app: &AppHandle) -> Result<(Core, data_lock::DataLock), Box<dyn std::error::Error>> {
    let dir = data_dir(app)?;
    std::fs::create_dir_all(&dir)?;
    let lock = data_lock::acquire(&dir)?;
    let model = std::env::var("YHTYE_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
    tracing::info!("data dir {}, model {model}", dir.display());
    let core = tauri::async_runtime::block_on(Core::start(CoreConfig::installed(&dir, &model)))?;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    webkit_env::apply();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![yhtye_command])
        .setup(|app| {
            let handle = app.handle().clone();
            let (core, lock) = match start_core(&handle) {
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
