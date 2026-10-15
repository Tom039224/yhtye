//! The Tauri shell (`core-design.md` §9): starts the [`Core`] with the
//! production configuration, forwards `yhtye_command` to it (or, given a host,
//! to another Yhtye this app is a client of: [`remote_hosts`]), emits every
//! event as `yhtye://event` (`{host, event}`), starts remote access if it is
//! on, connects to the hosts paired with before, and shuts every agent down
//! (and the connections and remote access) when the app exits.
//!
//! `main` first looks at the command line ([`cli`]): `--headless` runs the core
//! without any of Tauri ([`headless`]), `yhtye remote …` talks to such an
//! instance, and only the plain command opens the window.

pub mod cli;
pub mod data_dir;
mod data_lock;
mod deep_link;
#[cfg(unix)]
pub mod headless;
mod legacy_data;
mod remote_hosts;
mod webkit_env;

use std::io::IsTerminal;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, RunEvent, State};
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

/// The answer to [`yhtye_command`]: what the local core returned, or what a
/// remote host's `ok` was (JSON; both have a `type`).
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum CommandReply {
    Local(Box<ApiResponse>),
    Remote(Value),
}

/// The one command: runs an [`ApiCommand`] on the core (`host` is `None`) or
/// on the remote host with that `server_id`, which must be connected.
#[tauri::command]
async fn yhtye_command(
    core: State<'_, Core>,
    host: Option<String>,
    cmd: ApiCommand,
) -> Result<CommandReply, ApiError> {
    match host {
        None => core
            .command(cmd)
            .await
            .map(|reply| CommandReply::Local(Box::new(reply))),
        Some(server_id) => core
            .remote_hosts()
            .call(&server_id, &cmd)
            .await
            .map(CommandReply::Remote)
            .map_err(remote_hosts::api_error),
    }
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

/// Emits every event of the local core to the webview (`{host: null, event}`).
/// A lagging receiver skips events; the webview is then sent a `resync` hint
/// and catches up (the event log, the project list, the settings on display;
/// T-65).
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
            remote_hosts::emit_event(&app, None, &ev);
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

/// Whether a second `yhtye` hands over to the running app instead of starting
/// (`tauri-plugin-single-instance`; it recognises the app by its identifier
/// alone). Only with the default data directory: a named one (`--data-dir`,
/// `YHTYE_DATA_DIR`) is how a second app is run beside the first
/// ([`data_lock`]), and the one that holds the default directory is the one
/// the OS hands `yhtye://` links to.
#[cfg(desktop)]
fn is_single_instance(opts: &Options) -> bool {
    holds_default_dir(DataDir::resolve(opts.data_dir.as_deref()).ok().as_ref())
}

#[cfg(desktop)]
fn holds_default_dir(dir: Option<&DataDir>) -> bool {
    dir.is_some_and(|dir| dir.is_default)
}

/// Opens the window. Needs a display (GTK starts in `Builder::build`).
pub fn run_app(opts: &Options) {
    webkit_env::apply();
    init_tracing(false);
    let opts = opts.clone();
    let builder = tauri::Builder::default();
    // First: a second app must be found before anything else of the setup runs
    // (it would stop at the data directory lock otherwise, and its link would be lost).
    #[cfg(desktop)]
    let builder = if is_single_instance(&opts) {
        builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // The links in its arguments went to the deep-link plugin already.
            deep_link::focus_main_window(app);
        }))
    } else {
        builder
    };
    let app = builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            yhtye_command,
            remote_hosts::yhtye_remote_hosts,
            remote_hosts::yhtye_remote_preview_pairing,
            remote_hosts::yhtye_remote_add,
            remote_hosts::yhtye_remote_remove,
            remote_hosts::yhtye_remote_connect,
            deep_link::yhtye_pair_links_ready,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let (core, lock) = match start_core(&opts) {
                Ok(started) => started,
                Err(e) if e.is::<data_lock::LockError>() => {
                    // Not a crash: the other app is the one in use.
                    tracing::error!("{e}");
                    eprintln!("Yhtye: {e}");
                    #[cfg(desktop)]
                    tauri_plugin_single_instance::destroy(app);
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
            // The hosts this app is a client of: their events and states go to
            // the webview (subscribed before any connects), and the hosts paired
            // with before are connected. Their connections close with `Core::shutdown`.
            let hosts = core.remote_hosts();
            remote_hosts::forward_notices(handle.clone(), hosts.clone(), hosts.subscribe());
            tauri::async_runtime::spawn(async move {
                remote_hosts::connect_saved(&hosts).await;
            });
            app.manage(core);
            app.manage(deep_link::PairLinks::default());
            deep_link::listen(app);
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

#[cfg(all(test, desktop))]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn resolve(flag: Option<&str>, env: Option<&str>) -> Option<DataDir> {
        DataDir::resolve_from(
            flag.map(Path::new),
            env.map(Path::new),
            Some(PathBuf::from("/home/u/.local/share")),
        )
        .ok()
    }

    #[test]
    fn only_the_app_on_the_default_data_directory_takes_over_a_second_launch() {
        assert!(holds_default_dir(resolve(None, None).as_ref()));
        // A named directory is how a second app is run beside the first.
        assert!(!holds_default_dir(resolve(Some("/tmp/y"), None).as_ref()));
        assert!(!holds_default_dir(resolve(None, Some("/tmp/y")).as_ref()));
        // Empty counts as not named; no data directory at all is no reason to take over.
        assert!(holds_default_dir(resolve(Some(""), Some("")).as_ref()));
        assert!(!holds_default_dir(None));
    }
}
