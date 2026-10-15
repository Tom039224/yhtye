//! The other Yhtye hosts this app is a client of (`core-design.md` §9, §18.9),
//! between [`RemoteHosts`] (which pairs, connects and runs commands, with no
//! Tauri in it) and the webview: the commands that manage the hosts, the
//! events that tell what changed, and the events of the hosts themselves.
//!
//! The WebSocket to a host is made here, not in the webview: a WebView's
//! `WebSocket` cannot send an `Authorization` header, the CSP does not allow
//! `ws://100.x.y.z`, and the device token stays out of the page this way.
//!
//! Two events leave for the webview:
//! - `yhtye://event`: every event of every host as `{host, event}` (`host` is
//!   `null` for this app's own core, a `server_id` for a remote host). Each
//!   store of the frontend takes the ones of its own host.
//! - `yhtye://hosts`: what changed about the hosts themselves ([`HostNotice`]
//!   without the events: `hosts_changed`, `status`).

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::broadcast;
use yhtye_core::api::{ApiError, ApiErrorCode, ApiEvent, ApiEventBody};
use yhtye_core::remote_client::{
    parse_manual_entry_with, parse_pairing_uri_with, CallError, HostNotice, HostPolicy, HostsError,
    InvalidPairingInput, PairingFailure, PairingTarget, RemoteHostInfo, RemoteHosts,
};
use yhtye_core::runtime::{host_name, Core};

/// Event name of the events of every host (`src/api/tauri.ts`).
pub const EVENT: &str = "yhtye://event";
/// Event name of the changes of the host list and the hosts' states.
pub const HOSTS_EVENT: &str = "yhtye://hosts";

/// What `yhtye://event` carries: an event and whose it is.
#[derive(Serialize)]
pub struct EventEnvelope<'a, E: Serialize + ?Sized> {
    /// `None`: the local core. Else the `server_id` of the remote host.
    pub host: Option<&'a str>,
    pub event: &'a E,
}

// A borrow of the event and the host: copying it copies two references (what `emit` asks for).
impl<E: Serialize + ?Sized> Clone for EventEnvelope<'_, E> {
    fn clone(&self) -> Self {
        Self {
            host: self.host,
            event: self.event,
        }
    }
}

/// Emits `event` of `host` to the webview.
pub fn emit_event<E: Serialize + ?Sized>(app: &AppHandle, host: Option<&str>, event: &E) {
    if let Err(e) = app.emit(EVENT, EventEnvelope { host, event }) {
        tracing::warn!("could not emit an event: {e}");
    }
}

/// Passes everything [`RemoteHosts`] reports on to the webview, until it closes.
/// `rx` is made before any host connects, so that no notice is missed.
pub fn forward_notices(
    app: AppHandle,
    hosts: RemoteHosts,
    mut rx: broadcast::Receiver<HostNotice>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(HostNotice::Event { server_id, event }) => {
                    emit_event(&app, Some(&server_id), &*event);
                }
                Ok(notice) => emit_hosts(&app, &notice),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("the webview fell behind on remote hosts; {n} notices skipped");
                    catch_up_after_lag(&app, &hosts).await;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}

/// The host list or a state changed (not an event of a host: that goes to [`EVENT`]).
fn emit_hosts(app: &AppHandle, notice: &HostNotice) {
    if let Err(e) = app.emit(HOSTS_EVENT, notice) {
        tracing::warn!("could not emit a host notice: {e}");
    }
}

/// Notices were dropped, so the webview does not know which: it reads the host
/// list again, and every connected host's store catches up (`resync`, like the
/// local core's after a lag).
async fn catch_up_after_lag(app: &AppHandle, hosts: &RemoteHosts) {
    emit_hosts(app, &HostNotice::HostsChanged);
    let Ok(list) = hosts.list().await else { return };
    for info in list {
        let resync = ApiEvent::app_wide(ApiEventBody::Resync);
        emit_event(app, Some(&info.record.server_id), &resync);
    }
}

/// Connects to every host that was paired with before (at startup). One that
/// cannot be (its token is gone, ...) is logged and shown as not connected.
pub async fn connect_saved(hosts: &RemoteHosts) {
    let list = match hosts.list().await {
        Ok(list) => list,
        Err(e) => {
            tracing::warn!("could not read the remote hosts: {e}");
            return;
        }
    };
    for info in list {
        let id = &info.record.server_id;
        match hosts.connect(id).await {
            Ok(()) => tracing::info!("connecting to remote host {} ({id})", info.record.name),
            Err(e) => tracing::warn!(
                "not connecting to remote host {} ({id}): {e}",
                info.record.name
            ),
        }
    }
}

/// How a failure of [`RemoteHosts`] reaches the webview. A host's own error
/// (`NotFound` for a project that does not exist there, ...) passes unchanged.
pub fn api_error(e: HostsError) -> ApiError {
    let message = e.to_string();
    match e {
        HostsError::Call(CallError::Api(host_error)) => host_error,
        HostsError::Call(CallError::Encode(_)) => ApiError::internal(message),
        HostsError::Call(_) | HostsError::NotConnected(_) | HostsError::TokenMissing => {
            ApiError::unavailable(message)
        }
        HostsError::UnknownHost(_) => ApiError::not_found(message),
        HostsError::NotTailscaleHost(_) => ApiError::invalid_argument(message),
        HostsError::Pairing(failure) => pairing_error(&failure),
        HostsError::Credentials(_) | HostsError::Store(_) | HostsError::Corrupt(_) => {
            ApiError::internal(message)
        }
    }
}

fn pairing_error(failure: &PairingFailure) -> ApiError {
    let message = failure.to_string();
    match failure {
        PairingFailure::CodeRejected(_) | PairingFailure::OtherUser(_) => {
            ApiError::new(ApiErrorCode::Forbidden, message)
        }
        PairingFailure::BadRequest(_) => ApiError::invalid_argument(message),
        PairingFailure::WrongServer { .. } => ApiError::conflict(message),
        PairingFailure::Unreachable { .. } | PairingFailure::ServerError { .. } => {
            ApiError::unavailable(message)
        }
    }
}

/// Whom to pair with, as the webview gives it.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PairingInput {
    /// `yhtye://pair?…` (a deep link, a QR code, pasted).
    Uri { uri: String },
    /// What a person typed. `port` is text (empty: the default port).
    Manual {
        host: String,
        port: String,
        code: String,
    },
}

impl PairingInput {
    /// The target this names; only Tailscale hosts with `policy`
    /// [`HostPolicy::TailscaleOnly`].
    pub fn target(&self, policy: HostPolicy) -> Result<PairingTarget, ApiError> {
        match self {
            Self::Uri { uri } => parse_pairing_uri_with(uri, policy),
            Self::Manual { host, port, code } => parse_manual_entry_with(host, port, code, policy),
        }
        .map_err(|e: InvalidPairingInput| ApiError::invalid_argument(e.to_string()))
    }
}

/// What the webview shows before the person agrees to pair (never the code).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairingPreview {
    /// Where the host will be looked for, in order.
    pub hosts: Vec<String>,
    pub port: u16,
    /// Known when the input is a QR code / link.
    pub server_id: Option<String>,
    /// The name this device is offered to be listed under on the host.
    pub default_device_name: String,
}

impl PairingPreview {
    #[must_use]
    pub fn of(target: &PairingTarget, default_device_name: String) -> Self {
        Self {
            hosts: target.hosts().to_vec(),
            port: target.port(),
            server_id: target.server_id().map(str::to_string),
            default_device_name,
        }
    }
}

/// What [`yhtye_remote_remove`] could not undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoveOutcome {
    /// The credential store would not delete the token: a copy may be left there.
    pub token_left_in_keyring: Option<String>,
}

/// The host list, with each host's live state.
#[tauri::command]
pub async fn yhtye_remote_hosts(core: State<'_, Core>) -> Result<Vec<RemoteHostInfo>, ApiError> {
    core.remote_hosts().list().await.map_err(api_error)
}

/// What pairing with `input` would do: for the person to confirm (and name
/// this device) before [`yhtye_remote_add`]. Nothing is sent anywhere.
#[tauri::command]
pub fn yhtye_remote_preview_pairing(input: PairingInput) -> Result<PairingPreview, ApiError> {
    let target = input.target(HostPolicy::TailscaleOnly)?;
    Ok(PairingPreview::of(&target, host_name()))
}

/// Pairs with a host as a device called `device_name`, remembers it and
/// connects. A host paired with before is replaced.
#[tauri::command]
pub async fn yhtye_remote_add(
    core: State<'_, Core>,
    input: PairingInput,
    device_name: String,
) -> Result<RemoteHostInfo, ApiError> {
    let target = input.target(HostPolicy::TailscaleOnly)?;
    let hosts = core.remote_hosts();
    let info = hosts.add(&target, &device_name).await.map_err(api_error)?;
    // Paired is paired: a connection that fails to start shows as the host's state.
    if let Err(e) = hosts.connect(&info.record.server_id).await {
        tracing::warn!("not connecting to {}: {e}", info.record.server_id);
    }
    Ok(info)
}

/// Forgets a host (the host still lists this device until it is removed there).
#[tauri::command]
pub async fn yhtye_remote_remove(
    core: State<'_, Core>,
    server_id: String,
) -> Result<RemoveOutcome, ApiError> {
    let report = core
        .remote_hosts()
        .remove(&server_id)
        .await
        .map_err(api_error)?;
    Ok(RemoveOutcome {
        token_left_in_keyring: report.token_left_in_keyring,
    })
}

/// Connects to a host now (again after it was refused, or the wait between
/// attempts is to be skipped).
#[tauri::command]
pub async fn yhtye_remote_connect(
    core: State<'_, Core>,
    server_id: String,
) -> Result<(), ApiError> {
    core.remote_hosts()
        .connect(&server_id)
        .await
        .map_err(api_error)
}

#[cfg(test)]
mod tests;
