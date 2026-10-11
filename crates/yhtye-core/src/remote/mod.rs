//! Remote access over Tailscale (`core-design.md` §18; the Android client,
//! `docs/planned/android-client.md`): an HTTP + WebSocket server that listens
//! only on this PC's Tailscale addresses, pairs devices with one-time codes and
//! serves the API ([`crate::ws`]) to paired devices of the same Tailscale user.
//!
//! [`RemoteService`] belongs to the [`Core`]; it is driven by the local-only
//! commands ([`crate::api::ApiCommand::is_local_only`]) and reports changes as
//! app-wide events ([`ApiEventBody::RemoteStatusChanged`] /
//! [`ApiEventBody::RemoteDevicesChanged`]).

mod pairing;
mod server;
mod tailscale;

pub use pairing::{CODE_LEN, CODE_TTL_MS, MAX_FAILURES, token_hash};
pub use tailscale::{Tailscale, TailscaleCli, TailscaleError, TailscaleSelf, is_tailscale_address};

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::api::{ApiError, ApiEvent, ApiEventBody};
use crate::runtime::Core;
use crate::store::{DeviceRecord, Store};
use pairing::Pairing;

/// Default port of the remote server.
pub const DEFAULT_PORT: u16 = 1423;
/// Longest device name (characters).
pub const MAX_DEVICE_NAME_CHARS: usize = 64;

const SETTINGS_KEY: &str = "remote.settings";
const SERVER_ID_KEY: &str = "remote.server_id";
/// How long stopping waits for the listeners to finish.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// The stored settings of remote access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSettings {
    pub enabled: bool,
    pub port: u16,
}

impl Default for RemoteSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
        }
    }
}

/// What the settings panel shows of remote access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct RemoteStatus {
    /// The setting: remote access is wanted (it may still have failed to start).
    pub enabled: bool,
    /// The configured port.
    pub port: u16,
    /// The server is listening.
    pub running: bool,
    /// Where it listens (`100.x.y.z:1423`, `[fd7a:…]:1423`); empty when not running.
    pub addresses: Vec<String>,
    /// This PC's Tailscale IPv4 address (for the QR code), while running.
    pub ip: Option<String>,
    /// This PC's MagicDNS name (`pc.tailnet.ts.net`), while running and MagicDNS is on.
    pub dns_name: Option<String>,
    /// Identifies this Yhtye to devices (random, made once).
    pub server_id: String,
    /// Why it is not running although enabled (Tailscale missing or stopped,
    /// the port taken, ...).
    pub error: Option<String>,
    /// Expiry of the pairing code that is valid now, if any (milliseconds since
    /// the Unix epoch). The code itself is only in [`PairingCode`].
    pub pairing_expires_at_ms: Option<u64>,
}

/// A paired device as the settings panel lists it (never its token).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct RemoteDevice {
    pub id: String,
    pub name: String,
    pub created_ms: u64,
    /// When it last connected (`null`: never since pairing).
    pub last_seen_ms: Option<u64>,
    /// It is connected now.
    pub connected: bool,
}

/// A newly issued pairing code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct PairingCode {
    pub code: String,
    pub expires_at_ms: u64,
    /// The QR code's content:
    /// `yhtye://pair?v=1&host=<MagicDNS name>&ip=<100.x>&port=<N>&code=<code>&sid=<server id>`
    /// (`host` is left out without MagicDNS).
    pub uri: String,
}

/// State shared with the server's handlers.
#[derive(Debug)]
struct Shared {
    store: Store,
    tailscale: Arc<dyn Tailscale>,
    events: broadcast::Sender<ApiEvent>,
    server_id: String,
    pairing: Mutex<Pairing>,
    /// Open WebSocket connections by device; cancelling the token closes them.
    connections: Mutex<HashMap<String, DeviceConnections>>,
    /// Held while a connection is attached and while a device is revoked, so a
    /// connection can never outlive the revocation of its device.
    auth_gate: tokio::sync::Mutex<()>,
    /// The status last reported, for reporting a change of the pairing code
    /// from the server (which does not hold the server state).
    last_status: Mutex<Option<RemoteStatus>>,
    /// Numbers the servers started, so a connection of a stopped server that
    /// closes late does not count against the next one's.
    generation: AtomicU64,
}

#[derive(Debug)]
struct DeviceConnections {
    cancel: CancellationToken,
    count: usize,
    generation: u64,
}

#[derive(Debug, Default)]
struct ServerState {
    settings: RemoteSettings,
    running: Option<Running>,
    error: Option<String>,
}

#[derive(Debug)]
struct Running {
    addresses: Vec<SocketAddr>,
    me: TailscaleSelf,
    cancel: CancellationToken,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

/// Remote access of one [`Core`].
#[derive(Debug)]
pub struct RemoteService {
    shared: Arc<Shared>,
    /// Also serializes starting and stopping.
    state: tokio::sync::Mutex<ServerState>,
}

impl RemoteService {
    /// Reads the settings and the server id (made and stored on first use).
    /// Does not start the server; see [`RemoteService::start_if_enabled`].
    pub(crate) async fn new(
        store: Store,
        tailscale: Arc<dyn Tailscale>,
        events: broadcast::Sender<ApiEvent>,
    ) -> Result<Self, ApiError> {
        let id = pairing::random_hex(16)
            .map_err(|e| ApiError::internal(format!("no random server id: {e}")))?;
        let server_id = store
            .app_setting_or_insert(SERVER_ID_KEY, &id, now_ms())
            .await?;
        let settings = match store.app_setting(SETTINGS_KEY).await? {
            Some(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::error!("ignoring unreadable remote access settings: {e}");
                RemoteSettings::default()
            }),
            None => RemoteSettings::default(),
        };
        Ok(Self {
            shared: Arc::new(Shared {
                store,
                tailscale,
                events,
                server_id,
                pairing: Mutex::new(Pairing::default()),
                connections: Mutex::new(HashMap::new()),
                auth_gate: tokio::sync::Mutex::new(()),
                last_status: Mutex::new(None),
                generation: AtomicU64::new(0),
            }),
            state: tokio::sync::Mutex::new(ServerState {
                settings,
                ..ServerState::default()
            }),
        })
    }

    pub(crate) async fn status(&self) -> RemoteStatus {
        let state = self.state.lock().await;
        self.shared.status_of(&state)
    }

    /// Starts the server if the stored settings enable it (app start). A
    /// failure is kept in the status, not returned.
    pub(crate) async fn start_if_enabled(&self, core: &Core) {
        let mut state = self.state.lock().await;
        if state.settings.enabled && state.running.is_none() {
            self.start(&mut state, core).await;
            self.shared.emit_status(&state);
        }
    }

    /// Stores and applies new settings: starts, restarts (another port) or
    /// stops the server. Enabling it again when it failed to start retries.
    pub(crate) async fn set_settings(
        &self,
        core: &Core,
        enabled: bool,
        port: Option<u16>,
    ) -> Result<RemoteStatus, ApiError> {
        let mut state = self.state.lock().await;
        let port = port.unwrap_or(state.settings.port);
        if port == 0 {
            return Err(ApiError::invalid_argument("the port must be 1-65535"));
        }
        let settings = RemoteSettings { enabled, port };
        let text = serde_json::to_string(&settings)
            .map_err(|e| ApiError::internal(format!("remote settings: {e}")))?;
        self.shared
            .store
            .set_app_setting(SETTINGS_KEY, &text, now_ms())
            .await?;
        let changed = settings != state.settings;
        state.settings = settings;
        if changed || state.running.is_none() {
            self.stop_server(&mut state).await;
            if enabled {
                self.start(&mut state, core).await;
            }
        }
        self.shared.emit_status(&state);
        Ok(self.shared.status_of(&state))
    }

    /// Stops the server (the core is shutting down). The settings stay.
    pub(crate) async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        if state.running.is_some() {
            self.stop_server(&mut state).await;
            self.shared.emit_status(&state);
        }
    }

    /// A new pairing code (the old one stops working). The server must be running.
    pub(crate) async fn create_pairing_code(&self) -> Result<PairingCode, ApiError> {
        let state = self.state.lock().await;
        let Some(running) = &state.running else {
            return Err(ApiError::new(
                crate::api::ApiErrorCode::InvalidState,
                "remote access is not running",
            ));
        };
        let (code, expires_at_ms) = self
            .shared
            .lock_pairing()
            .issue(now_ms())
            .map_err(|e| ApiError::internal(format!("no random pairing code: {e}")))?;
        let uri = pairing_uri(running, &code, &self.shared.server_id);
        self.shared.emit_status(&state);
        Ok(PairingCode {
            code,
            expires_at_ms,
            uri,
        })
    }

    pub(crate) async fn devices(&self) -> Result<Vec<RemoteDevice>, ApiError> {
        self.shared.devices().await
    }

    /// Forgets a device and closes its connections at once.
    pub(crate) async fn revoke(&self, device: &str) -> Result<Vec<RemoteDevice>, ApiError> {
        {
            let _gate = self.shared.auth_gate.lock().await;
            if !self.shared.store.remove_remote_device(device).await? {
                return Err(ApiError::not_found(format!("unknown device {device}")));
            }
            if let Some(conns) = self.shared.lock_connections().remove(device) {
                conns.cancel.cancel();
            }
        }
        tracing::info!("revoked remote device {device}");
        let devices = self.shared.devices().await?;
        self.shared.emit(ApiEventBody::RemoteDevicesChanged {
            devices: devices.clone(),
        });
        Ok(devices)
    }

    /// Binds every Tailscale address and serves on them; on failure records
    /// the error and leaves nothing listening.
    async fn start(&self, state: &mut ServerState, core: &Core) {
        state.error = None;
        match self.bind(state.settings.port).await {
            Ok((listeners, me)) => {
                let cancel = CancellationToken::new();
                let ctx = server::ServerCtx {
                    shared: Arc::clone(&self.shared),
                    core: core.clone(),
                    self_user: me.user_id,
                    cancel: cancel.clone(),
                    generation: self.shared.generation.fetch_add(1, Ordering::Relaxed) + 1,
                };
                let addresses: Vec<SocketAddr> = listeners
                    .iter()
                    .filter_map(|l| l.local_addr().ok())
                    .collect();
                let tasks = listeners
                    .into_iter()
                    .map(|l| tokio::spawn(server::serve(l, ctx.clone())))
                    .collect();
                tracing::info!("remote access listening on {addresses:?}");
                state.running = Some(Running {
                    addresses,
                    me,
                    cancel,
                    tasks,
                });
            }
            Err(e) => {
                tracing::warn!("remote access could not start: {e}");
                state.error = Some(e);
            }
        }
    }

    async fn bind(
        &self,
        port: u16,
    ) -> Result<(Vec<tokio::net::TcpListener>, TailscaleSelf), String> {
        let me = self.shared.tailscale.status().await.map_err(|e| e.0)?;
        let mut listeners = Vec::new();
        let mut port = port;
        for (i, ip) in me.addresses.iter().enumerate() {
            let addr = SocketAddr::new(*ip, port);
            match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => {
                    // Port 0 (tests): the other addresses take the same port.
                    port = l.local_addr().map_or(port, |a| a.port());
                    listeners.push(l);
                }
                // The first address (IPv4) must work; IPv6 is a bonus.
                Err(e) if i == 0 => return Err(format!("cannot listen on {addr}: {e}")),
                Err(e) => tracing::warn!("remote access: cannot also listen on {addr}: {e}"),
            }
        }
        Ok((listeners, me))
    }

    async fn stop_server(&self, state: &mut ServerState) {
        let Some(running) = state.running.take() else {
            return;
        };
        running.cancel.cancel();
        for mut task in running.tasks {
            if tokio::time::timeout(STOP_TIMEOUT, &mut task).await.is_err() {
                tracing::warn!("a remote access listener did not stop in time");
                task.abort();
            }
        }
        self.shared.lock_pairing().clear();
        self.shared.lock_connections().clear();
        tracing::info!("remote access stopped");
    }
}

impl Shared {
    fn status_of(&self, state: &ServerState) -> RemoteStatus {
        let running = state.running.as_ref();
        RemoteStatus {
            enabled: state.settings.enabled,
            port: state.settings.port,
            running: running.is_some(),
            addresses: running
                .map(|r| r.addresses.iter().map(ToString::to_string).collect())
                .unwrap_or_default(),
            ip: running.and_then(|r| ipv4(&r.me)).map(|ip| ip.to_string()),
            dns_name: running.and_then(|r| r.me.dns_name.clone()),
            server_id: self.server_id.clone(),
            error: state.error.clone(),
            pairing_expires_at_ms: self.lock_pairing().expires_at_ms(now_ms()),
        }
    }

    fn emit_status(&self, state: &ServerState) {
        let status = self.status_of(state);
        *self
            .last_status
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(status.clone());
        self.emit(ApiEventBody::RemoteStatusChanged { status });
    }

    /// Reports the status again if the pairing code was used up or dropped.
    fn emit_pairing_change(&self) {
        let status = {
            let mut last = self
                .last_status
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let Some(status) = last.as_mut() else {
                return;
            };
            let expires = self.lock_pairing().expires_at_ms(now_ms());
            if status.pairing_expires_at_ms == expires {
                return;
            }
            status.pairing_expires_at_ms = expires;
            status.clone()
        };
        self.emit(ApiEventBody::RemoteStatusChanged { status });
    }

    /// An app-wide event: not stored, no project (see [`ApiEventBody::is_local_only`]).
    fn emit(&self, body: ApiEventBody) {
        let _ = self.events.send(ApiEvent {
            seq: 0,
            ts_ms: now_ms(),
            project: String::new(),
            live: true,
            body,
        });
    }

    async fn devices(&self) -> Result<Vec<RemoteDevice>, ApiError> {
        let records = self.store.remote_devices().await?;
        let conns = self.lock_connections();
        Ok(records
            .into_iter()
            .map(|r| RemoteDevice {
                connected: conns.contains_key(&r.id),
                id: r.id,
                name: r.name,
                created_ms: r.created_ms,
                last_seen_ms: r.last_seen_ms,
            })
            .collect())
    }

    async fn emit_devices(&self) {
        match self.devices().await {
            Ok(devices) => self.emit(ApiEventBody::RemoteDevicesChanged { devices }),
            Err(e) => tracing::warn!("could not list the remote devices: {e}"),
        }
    }

    /// The device whose token this is. Every stored hash is compared in
    /// constant time (no early exit).
    async fn device_by_token(&self, token: &str) -> Result<Option<DeviceRecord>, ApiError> {
        let hash = token_hash(token);
        let mut found = None;
        for d in self.store.remote_devices().await? {
            if pairing::constant_time_eq(d.token_hash.as_bytes(), hash.as_bytes()) {
                found = Some(d);
            }
        }
        Ok(found)
    }

    /// Registers a connection of `device` (still stored, checked under the
    /// gate); its token is cancelled when the device is revoked or the server stops.
    async fn attach(
        &self,
        device: &str,
        server: &CancellationToken,
        generation: u64,
    ) -> Option<CancellationToken> {
        let _gate = self.auth_gate.lock().await;
        let exists = match self.store.remote_devices().await {
            Ok(all) => all.iter().any(|d| d.id == device),
            Err(e) => {
                tracing::warn!("could not check the remote device {device}: {e}");
                false
            }
        };
        if !exists || server.is_cancelled() {
            return None;
        }
        let mut conns = self.lock_connections();
        let entry = conns
            .entry(device.to_string())
            .or_insert_with(|| DeviceConnections {
                cancel: server.child_token(),
                count: 0,
                generation,
            });
        if entry.generation != generation {
            // Left over from a stopped server (its token is cancelled).
            *entry = DeviceConnections {
                cancel: server.child_token(),
                count: 0,
                generation,
            };
        }
        entry.count += 1;
        Some(entry.cancel.child_token())
    }

    fn detach(&self, device: &str, generation: u64) {
        let mut conns = self.lock_connections();
        if let Some(entry) = conns.get_mut(device).filter(|e| e.generation == generation) {
            entry.count = entry.count.saturating_sub(1);
            if entry.count == 0 {
                conns.remove(device);
            }
        }
    }

    /// Pairs a new device: stores it (with the token's hash) and returns its
    /// id and token.
    async fn add_device(&self, name: String) -> Result<(String, String), ApiError> {
        let random = |n| {
            pairing::random_hex(n).map_err(|e| ApiError::internal(format!("no randomness: {e}")))
        };
        let id = format!("dev-{}", random(8)?);
        let token = pairing::new_token()
            .map_err(|e| ApiError::internal(format!("no random token: {e}")))?;
        self.store
            .add_remote_device(&DeviceRecord {
                id: id.clone(),
                name,
                token_hash: token_hash(&token),
                created_ms: now_ms(),
                last_seen_ms: None,
            })
            .await?;
        Ok((id, token))
    }

    fn lock_pairing(&self) -> std::sync::MutexGuard<'_, Pairing> {
        self.pairing.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_connections(&self) -> std::sync::MutexGuard<'_, HashMap<String, DeviceConnections>> {
        self.connections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

fn ipv4(me: &TailscaleSelf) -> Option<IpAddr> {
    me.addresses
        .iter()
        .copied()
        .find(IpAddr::is_ipv4)
        .or_else(|| me.addresses.first().copied())
}

fn pairing_uri(running: &Running, code: &str, server_id: &str) -> String {
    let port = running.addresses.first().map_or(0, SocketAddr::port);
    let mut uri = String::from("yhtye://pair?v=1");
    if let Some(host) = &running.me.dns_name {
        uri.push_str(&format!("&host={}", query_escape(host)));
    }
    if let Some(ip) = ipv4(&running.me) {
        uri.push_str(&format!("&ip={}", query_escape(&ip.to_string())));
    }
    uri.push_str(&format!("&port={port}&code={code}&sid={server_id}"));
    uri
}

/// Percent-encodes everything but unreserved characters (RFC 3986).
fn query_escape(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// A device name as given at pairing: trimmed, control characters dropped,
/// 1 to [`MAX_DEVICE_NAME_CHARS`] characters.
fn device_name(name: &str) -> Result<String, String> {
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    let name = name.trim();
    if name.is_empty() {
        return Err("the device name is empty".into());
    }
    if name.chars().count() > MAX_DEVICE_NAME_CHARS {
        return Err(format!(
            "the device name is longer than {MAX_DEVICE_NAME_CHARS} characters"
        ));
    }
    Ok(name.to_string())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pairing_uri_carries_both_hosts_the_port_code_and_server_id() {
        let running = Running {
            addresses: vec!["100.89.117.15:1423".parse().unwrap()],
            me: TailscaleSelf {
                addresses: vec![
                    "fd7a:115c:a1e0::1".parse().unwrap(),
                    "100.89.117.15".parse().unwrap(),
                ],
                dns_name: Some("pc.tail1234.ts.net".into()),
                user_id: 1,
            },
            cancel: CancellationToken::new(),
            tasks: Vec::new(),
        };
        assert_eq!(
            pairing_uri(&running, "ABCD2345", "sid1"),
            "yhtye://pair?v=1&host=pc.tail1234.ts.net&ip=100.89.117.15&port=1423&code=ABCD2345&sid=sid1"
        );
        let mut no_dns = running;
        no_dns.me.dns_name = None;
        assert!(!pairing_uri(&no_dns, "C", "s").contains("host="));
    }

    #[test]
    fn device_names_are_trimmed_and_bounded() {
        assert_eq!(device_name("  Pixel 9\n").unwrap(), "Pixel 9");
        assert!(device_name(" \t").is_err());
        assert!(device_name(&"x".repeat(MAX_DEVICE_NAME_CHARS)).is_ok());
        assert!(device_name(&"x".repeat(MAX_DEVICE_NAME_CHARS + 1)).is_err());
    }

    #[test]
    fn query_values_are_escaped() {
        assert_eq!(query_escape("fd7a::1"), "fd7a%3A%3A1");
        assert_eq!(query_escape("a.b-c"), "a.b-c");
    }
}
