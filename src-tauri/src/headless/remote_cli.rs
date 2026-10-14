//! `yhtye remote pair | devices | revoke <id> | status | stop`: thin clients of a
//! running headless instance, over its control socket ([`super::control`]).
//! They start no GUI and no core.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use yhtye_core::api::ApiCommand;
use yhtye_core::remote::RemoteStatus;

use super::control::{socket_path, Client, ClientError, Response};
use super::{now_ms, report, say};
use crate::cli::RemoteAction;
use crate::data_dir::DataDir;

/// How long `remote stop` waits for the instance to end (it first stops every
/// agent; the same as the systemd unit's `TimeoutStopSec`).
const STOP_TIMEOUT: Duration = Duration::from_secs(120);

/// How often `remote stop` looks whether the instance has ended.
const STOP_POLL: Duration = Duration::from_millis(100);

pub fn run(action: &RemoteAction, data_dir: Option<PathBuf>) -> ExitCode {
    match run_action(action, data_dir) {
        Ok(text) => {
            // A closed stdout (`| head`) is not an error.
            let _ = std::io::stdout().write_all(text.as_bytes());
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("yhtye: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run_action(action: &RemoteAction, data_dir: Option<PathBuf>) -> Result<String, String> {
    let dir = DataDir::resolve(data_dir.as_deref()).map_err(|e| e.to_string())?;
    let socket = socket_path(&dir.path);
    let mut client = Client::connect(&socket).map_err(|e| e.to_string())?;
    let result = match action {
        RemoteAction::Pair => pair(&mut client),
        RemoteAction::Devices => devices(&mut client),
        RemoteAction::Revoke(id) => revoke(&mut client, id),
        RemoteAction::Status => status(&mut client, &dir),
        RemoteAction::Stop => stop(client, &socket),
    };
    result.map_err(|e| e.to_string())
}

fn unexpected(what: &str) -> ClientError {
    ClientError::Protocol(format!("not the answer of {what}"))
}

fn remote_status(client: &mut Client) -> Result<RemoteStatus, ClientError> {
    match client.call(&ApiCommand::GetRemoteStatus)? {
        Response::RemoteStatus { status } => Ok(status),
        _ => Err(unexpected("get_remote_status")),
    }
}

fn status(client: &mut Client, dir: &DataDir) -> Result<String, ClientError> {
    let status = report::status(&remote_status(client)?, now_ms());
    Ok(report::instance(client.server_pid(), &dir.path) + &status)
}

/// Asks the instance to stop and waits until it has: it ends after
/// `Core::shutdown` has stopped every agent.
fn stop(mut client: Client, socket: &std::path::Path) -> Result<String, ClientError> {
    let pid = client.server_pid();
    client.shutdown()?;
    drop(client);
    say(&match pid {
        Some(pid) => format!("Stopping Yhtye (pid {pid}); waiting for its agents to stop…"),
        None => "Stopping Yhtye; waiting for its agents to stop…".to_string(),
    });
    let deadline = Instant::now() + STOP_TIMEOUT;
    while is_alive(pid, socket) {
        if Instant::now() >= deadline {
            let who = pid.map_or_else(|| "it".to_string(), |pid| format!("pid {pid}"));
            return Err(ClientError::Protocol(format!(
                "Yhtye did not end within {} s ({who} is still stopping; it ends when its agents \
                 have stopped; `kill -9` forces it)",
                STOP_TIMEOUT.as_secs()
            )));
        }
        std::thread::sleep(STOP_POLL);
    }
    Ok("Stopped.\n".into())
}

/// The instance has not ended: its process exists (or, where the system cannot
/// name it, its socket still answers).
fn is_alive(pid: Option<u32>, socket: &std::path::Path) -> bool {
    match pid {
        Some(pid) => process_exists(pid),
        None => Client::connect(socket).is_ok(),
    }
}

fn process_exists(pid: u32) -> bool {
    let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return false;
    };
    match rustix::process::test_kill_process(pid) {
        Ok(()) => true,
        // It exists, but is not ours to signal.
        Err(e) => e == rustix::io::Errno::PERM,
    }
}

fn devices(client: &mut Client) -> Result<String, ClientError> {
    match client.call(&ApiCommand::ListRemoteDevices)? {
        Response::RemoteDevices { devices } => Ok(report::devices(&devices)),
        _ => Err(unexpected("list_remote_devices")),
    }
}

fn revoke(client: &mut Client, id: &str) -> Result<String, ClientError> {
    let cmd = ApiCommand::RevokeRemoteDevice {
        device: id.to_string(),
    };
    match client.call(&cmd)? {
        Response::RemoteDevices { devices } => Ok(format!(
            "Revoked {id}; its connections are closed ({} paired device{} left).\n",
            devices.len(),
            if devices.len() == 1 { "" } else { "s" }
        )),
        _ => Err(unexpected("revoke_remote_device")),
    }
}

/// Issues a code. While remote access is not running (Tailscale down, the port
/// taken) there is none: the status says why.
fn pair(client: &mut Client) -> Result<String, ClientError> {
    let pairing = match client.call(&ApiCommand::CreatePairingCode) {
        Ok(Response::PairingCode { pairing }) => pairing,
        Ok(_) => return Err(unexpected("create_pairing_code")),
        Err(ClientError::Api(e)) => return Err(ClientError::Api(explain(client, e))),
        Err(e) => return Err(e),
    };
    // After the code: the status then names the same address.
    let status = remote_status(client)?;
    Ok(report::pairing(&pairing, &status, now_ms()))
}

/// Adds the reason remote access is not running to the error.
fn explain(client: &mut Client, mut error: yhtye_core::api::ApiError) -> yhtye_core::api::ApiError {
    if let Ok(status) = remote_status(client) {
        if let Some(why) = status.error {
            error.message = format!("{} ({why})", error.message);
        } else if !status.enabled {
            error.message = format!("{} (it is turned off)", error.message);
        }
    }
    error
}
