//! `yhtye --headless` (`core-design.md` §9.4): the core and remote access
//! without a GUI, for a machine with no display (a server the Android client
//! works on). It starts before anything of Tauri does (no GTK, no WebKit): the
//! binary is the same as the app's, which only links the libraries.
//!
//! Start: the data directory (the same rule as the app's), its lock (taken:
//! exit code [`EXIT_LOCK_TAKEN`]), the core on a runtime of its own, interrupted
//! work resumed, remote access turned on and kept on ([`keep_remote_running`]:
//! Tailscale may come up after this process), and the control socket
//! ([`control`]) for `yhtye remote …`. Logs go to stderr; stdout carries the
//! human-readable "ready" lines, and on a terminal the pairing guidance (no
//! valid code is ever written where a service manager keeps its log).
//! SIGINT / SIGTERM / SIGHUP stop every agent before exiting.

mod control;
mod daemon;
mod remote_cli;
mod report;

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::signal::unix::{signal, Signal, SignalKind};
use tokio::sync::{oneshot, Notify};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use yhtye_core::api::{ApiCommand, ApiResponse};
use yhtye_core::remote::RemoteStatus;
use yhtye_core::runtime::{Core, CoreConfig};
use yhtye_core::secrets::SecretBackend;

use crate::cli::{Daemon, Options};
use crate::data_dir::{self, DataDir};
use crate::data_lock::{self, LockError};

pub use remote_cli::run as run_remote;

/// Exit code when another Yhtye holds the data directory. A service manager
/// must not restart it (`RestartPreventExitStatus=3`).
pub const EXIT_LOCK_TAKEN: u8 = 3;

/// How often remote access is checked and, when it is not running, started again.
const REMOTE_RETRY: Duration = Duration::from_secs(30);

/// How long the runtime waits for blocking calls (a keyring that does not
/// answer) after the core has stopped.
const RUNTIME_EXIT_GRACE: Duration = Duration::from_secs(2);

/// A name that is never stored: reading it tells whether a credential store answers.
const KEYRING_PROBE: &str = "headless-probe";

/// Runs the headless instance until a signal stops it.
pub fn run(opts: Options) -> ExitCode {
    if opts.daemon == Daemon::Launch {
        return daemon::launch(&opts);
    }
    crate::init_tracing(true);
    if opts.daemon == Daemon::Child {
        daemon::detach_from_terminal();
    }
    let dir = match DataDir::resolve(opts.data_dir.as_deref()).and_then(|d| d.prepare().map(|()| d))
    {
        Ok(dir) => dir,
        Err(e) => return fail(&e),
    };
    let lock = match data_lock::acquire(&dir.path) {
        Ok(lock) => lock,
        Err(e @ LockError::Taken(_)) => {
            eprintln!("Yhtye: {e}");
            return ExitCode::from(EXIT_LOCK_TAKEN);
        }
        Err(e) => return fail(&e),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return fail(&e),
    };
    let code = runtime.block_on(serve(&dir.path, &opts));
    runtime.shutdown_timeout(RUNTIME_EXIT_GRACE);
    drop(lock);
    code
}

fn fail(e: &dyn std::fmt::Display) -> ExitCode {
    eprintln!("Yhtye: {e}");
    ExitCode::FAILURE
}

/// Writes a line of the human-readable output (a closed stdout is not an error).
fn say(line: &str) {
    let _ = writeln!(std::io::stdout(), "{line}");
}

async fn serve(data_dir: &Path, opts: &Options) -> ExitCode {
    let daemon = opts.daemon == Daemon::Child;
    // Before the (slow) start: a signal then stops it cleanly instead of killing it.
    // A daemon has no terminal to close: SIGHUP is not a request to stop it.
    let mut signals = match Signals::listen(!daemon) {
        Ok(s) => s,
        Err(e) => return fail(&format!("cannot listen for signals: {e}")),
    };
    let model = data_dir::model(opts.model.as_deref());
    let mut cfg = CoreConfig::installed(data_dir, &model);
    cfg.headless = true;
    let secret_backend = cfg.secret_backend.clone();
    let core = match Core::start(cfg).await {
        Ok(core) => core,
        Err(e) => return fail(&format!("cannot start the core: {e}")),
    };
    tracing::info!("headless: data dir {}, model {model}", data_dir.display());
    warn_without_secret_store(secret_backend);

    let cancel = CancellationToken::new();
    let stop = Arc::new(Notify::new());
    let socket = control::socket_path(data_dir);
    let control = start_control(&socket, &core, &stop, &cancel);
    if control.is_none() && daemon {
        // Nothing could reach a daemon without its socket: not even stop it.
        core.shutdown().await;
        return fail(&"cannot open the control socket (see the log)");
    }
    say(&format!(
        "Yhtye (headless) is running on {} (pid {}). {}",
        data_dir.display(),
        std::process::id(),
        if daemon {
            "Stop it with `yhtye remote stop`."
        } else {
            "Stop it with Ctrl+C or SIGTERM."
        }
    ));
    if control.is_some() {
        say(&format!(
            "Control socket {} (`yhtye remote pair | devices | revoke <id> | status | stop`).",
            socket.display()
        ));
    }
    // Interrupted work resumes without waiting for a client to open the project.
    let resume = core.clone();
    tokio::spawn(async move {
        resume.resume_unfinished().await;
    });
    // After the lines above, so that "remote access is listening" follows them.
    let (listening_tx, listening_rx) = oneshot::channel();
    tokio::spawn(keep_remote_running(
        core.clone(),
        opts.port,
        REMOTE_RETRY,
        listening_tx,
        cancel.clone(),
    ));
    // No pairing code for a daemon: nobody reads its log as a terminal.
    let interactive = !daemon && std::io::stdout().is_terminal();
    tokio::spawn(announce_remote(core.clone(), listening_rx, interactive));

    tokio::select! {
        signal = signals.recv() => tracing::info!("{signal} received; shutting down every agent"),
        () = stop.notified() => tracing::info!("stop requested on the control socket; shutting down every agent"),
    }
    cancel.cancel();
    // A second signal does not wait for the agents.
    let forced = tokio::spawn(async move {
        let again = signals.recv().await;
        tracing::warn!("{again} again: exiting without waiting for the agents");
        std::process::exit(1);
    });
    if let Some(control) = control {
        let _ = control.await;
        let _ = std::fs::remove_file(&socket);
    }
    core.shutdown().await;
    forced.abort();
    tracing::info!("all agents stopped");
    ExitCode::SUCCESS
}

/// Starts answering `yhtye remote …`. Without the socket the rest still works.
fn start_control(
    socket: &Path,
    core: &Core,
    stop: &Arc<Notify>,
    cancel: &CancellationToken,
) -> Option<JoinHandle<()>> {
    match control::bind(socket) {
        Ok(listener) => Some(tokio::spawn(control::serve(
            listener,
            core.clone(),
            stop.clone(),
            cancel.clone(),
        ))),
        Err(e) => {
            tracing::error!(
                "cannot listen on the control socket {}: {e} (`yhtye remote …` will not work)",
                socket.display()
            );
            None
        }
    }
}

/// Secret environment variables live in the OS credential store; a server
/// without a Secret Service keyring cannot keep them. Only a warning: there is
/// no other place for them.
fn warn_without_secret_store(backend: Arc<dyn SecretBackend>) {
    // The call may block on D-Bus.
    tokio::task::spawn_blocking(move || {
        if let Err(e) = backend.get(KEYRING_PROBE) {
            tracing::warn!(
                "no secret store ({e}): secret environment variables cannot be saved or given to \
                 the agents (a Secret Service keyring such as gnome-keyring is needed)"
            );
        }
    });
}

/// The signals that stop the process.
struct Signals {
    interrupt: Signal,
    terminate: Signal,
    hangup: Signal,
    /// SIGHUP stops the process (it does not, for a daemon).
    hangup_stops: bool,
}

impl Signals {
    /// Listening replaces the signals' default action (ending the process),
    /// also for SIGHUP when it does not stop the process.
    fn listen(hangup_stops: bool) -> std::io::Result<Self> {
        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
            hangup: signal(SignalKind::hangup())?,
            hangup_stops,
        })
    }

    /// The name of the next signal that stops the process.
    async fn recv(&mut self) -> &'static str {
        loop {
            tokio::select! {
                _ = self.interrupt.recv() => return "SIGINT",
                _ = self.terminate.recv() => return "SIGTERM",
                _ = self.hangup.recv() => {
                    if self.hangup_stops {
                        return "SIGHUP";
                    }
                    tracing::info!("SIGHUP ignored");
                }
            }
        }
    }
}

/// Turns remote access on and keeps it on: it fails to start while Tailscale
/// is not up (a user service cannot be ordered after the system's `tailscaled`),
/// so it is tried again every `retry` until it listens, and again whenever it
/// stops running. The status of the first time it listens goes to `listening`.
async fn keep_remote_running(
    core: Core,
    port: Option<u16>,
    retry: Duration,
    listening: oneshot::Sender<RemoteStatus>,
    cancel: CancellationToken,
) {
    let mut listening = Some(listening);
    let mut checked = false;
    let mut last_problem: Option<String> = None;
    loop {
        match ensure_remote(&core, port, checked).await {
            Ok(status) => {
                last_problem = None;
                if let Some(listening) = listening.take() {
                    let _ = listening.send(status);
                }
            }
            Err(problem) => {
                // The same problem is told once, not on every try.
                if last_problem.as_deref() != Some(&problem) {
                    tracing::warn!(
                        "remote access is not running: {problem} (trying again every {} s)",
                        retry.as_secs()
                    );
                }
                last_problem = Some(problem);
            }
        }
        checked = true;
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(retry) => {}
        }
    }
}

/// The status of remote access when it is running, started if need be
/// (`check_first`: left alone while it runs); else why it is not.
async fn ensure_remote(
    core: &Core,
    port: Option<u16>,
    check_first: bool,
) -> Result<RemoteStatus, String> {
    if check_first {
        if let Ok(ApiResponse::RemoteStatus { status }) =
            core.command(ApiCommand::GetRemoteStatus).await
        {
            if status.running {
                return Ok(status);
            }
        }
    }
    let enable = ApiCommand::SetRemoteSettings {
        enabled: true,
        port,
    };
    match core.command(enable).await {
        Ok(ApiResponse::RemoteStatus { status }) if status.running => Ok(status),
        Ok(ApiResponse::RemoteStatus { status }) => Err(status
            .error
            .unwrap_or_else(|| "it did not start".to_string())),
        Ok(_) => Err("unexpected answer to set_remote_settings".into()),
        Err(e) => Err(e.message),
    }
}

/// Says that remote access listens (once it does); `interactive` (stdout is a
/// terminal, and this is not a daemon) also issues a pairing code and shows it.
/// Not otherwise: a service manager or a daemon keeps stdout in a log, and a
/// valid code does not belong there.
async fn announce_remote(
    core: Core,
    listening: oneshot::Receiver<RemoteStatus>,
    interactive: bool,
) {
    let Ok(status) = listening.await else {
        return;
    };
    let addresses = status.addresses.join(", ");
    tracing::info!("remote access is listening on {addresses}");
    say(&format!("Remote access is listening on {addresses}."));
    if !interactive {
        return;
    }
    match core.command(ApiCommand::CreatePairingCode).await {
        Ok(ApiResponse::PairingCode { pairing }) => {
            say("");
            say(&report::pairing(&pairing, &status, now_ms()));
            say("(`yhtye remote pair` shows a new code.)");
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("could not issue a pairing code: {e}"),
    }
}

/// Milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait;
    use yhtye_core::remote::{Tailscale, TailscaleError, TailscalePeer, TailscaleSelf};
    use yhtye_core::secrets::MemoryBackend;

    use super::*;

    const RETRY: Duration = Duration::from_millis(30);
    const WAIT: Duration = Duration::from_secs(20);

    /// Tailscale that is down until [`FakeTailscale::bring_up`].
    #[derive(Debug, Default)]
    struct FakeTailscale {
        up: AtomicBool,
    }

    impl FakeTailscale {
        fn bring_up(&self) {
            self.up.store(true, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl Tailscale for FakeTailscale {
        async fn status(&self) -> Result<TailscaleSelf, TailscaleError> {
            if !self.up.load(Ordering::SeqCst) {
                return Err(TailscaleError("tailscaled is not running".into()));
            }
            Ok(TailscaleSelf {
                addresses: vec![IpAddr::from([127, 0, 0, 1])],
                dns_name: None,
                user_id: 1,
                tagged: false,
            })
        }

        async fn whois(&self, _: IpAddr) -> Result<TailscalePeer, TailscaleError> {
            Ok(TailscalePeer {
                user_id: 1,
                tagged: false,
            })
        }
    }

    struct Keeper {
        core: Core,
        tailscale: Arc<FakeTailscale>,
        listening: oneshot::Receiver<RemoteStatus>,
        cancel: CancellationToken,
        task: JoinHandle<()>,
        port: u16,
        _tmp: tempfile::TempDir,
    }

    async fn keeper() -> Keeper {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tailscale = Arc::new(FakeTailscale::default());
        let mut cfg = CoreConfig::claude_code(tmp.path(), "haiku");
        cfg.usage = None;
        cfg.secret_backend = Arc::new(MemoryBackend::default());
        cfg.tailscale = tailscale.clone();
        let core = Core::start(cfg).await.expect("core");
        // A port nobody holds.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("a free port")
            .port();
        let (tx, listening) = oneshot::channel();
        let cancel = CancellationToken::new();
        let task = tokio::spawn(keep_remote_running(
            core.clone(),
            Some(port),
            RETRY,
            tx,
            cancel.clone(),
        ));
        Keeper {
            core,
            tailscale,
            listening,
            cancel,
            task,
            port,
            _tmp: tmp,
        }
    }

    impl Keeper {
        async fn status(&self) -> RemoteStatus {
            match self.core.command(ApiCommand::GetRemoteStatus).await {
                Ok(ApiResponse::RemoteStatus { status }) => status,
                other => panic!("a status: {other:?}"),
            }
        }

        async fn stop(self) {
            self.cancel.cancel();
            self.task.await.expect("the keeper ends when cancelled");
            self.core.shutdown().await;
        }
    }

    async fn until<F: std::future::Future<Output = bool>>(what: &str, mut done: impl FnMut() -> F) {
        let deadline = tokio::time::Instant::now() + WAIT;
        while !done().await {
            assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn remote_access_is_tried_again_until_tailscale_is_up() {
        let mut k = keeper().await;
        // Down: enabled, not running, and the reason is the status's.
        until("the first try failed", || async {
            k.status().await.error.is_some()
        })
        .await;
        let status = k.status().await;
        assert!(status.enabled && !status.running, "{status:?}");
        assert!(
            status
                .error
                .as_deref()
                .unwrap_or("")
                .contains("tailscaled is not running"),
            "{status:?}"
        );
        assert!(k.listening.try_recv().is_err(), "nothing listens yet");

        k.tailscale.bring_up();
        let listening = tokio::time::timeout(WAIT, &mut k.listening)
            .await
            .expect("it listens within the wait")
            .expect("the status");
        assert!(listening.running);
        assert_eq!(listening.port, k.port);
        assert_eq!(k.status().await.error, None);
        k.stop().await;
    }

    #[tokio::test]
    async fn remote_access_that_stopped_is_turned_on_again() {
        let mut k = keeper().await;
        k.tailscale.bring_up();
        tokio::time::timeout(WAIT, &mut k.listening)
            .await
            .expect("it listens")
            .expect("the status");
        // Something (a client of the control socket) turns it off.
        let off = ApiCommand::SetRemoteSettings {
            enabled: false,
            port: None,
        };
        k.core.command(off).await.expect("off");
        assert!(!k.status().await.running);
        until("the keeper turned it on again", || async {
            k.status().await.running
        })
        .await;
        k.stop().await;
    }
}
