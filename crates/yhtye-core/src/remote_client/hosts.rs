//! [`RemoteHosts`]: the Yhtye hosts (another PC, a headless Yhtye) this app is
//! paired with, and the connections to them.
//!
//! What is kept where:
//! - the list of hosts (who, where, which device id): in the local core's
//!   database (`app_settings`, key [`HOSTS_KEY`]). Nothing in it is secret.
//! - the device token: in the OS credential store ([`TokenVault`]); in memory
//!   for this process only if that fails ([`TokenState::MemoryOnly`], so the UI
//!   can say that the host must be paired again after a restart).
//!
//! Using it (a [`crate::runtime::Core`] has one: [`Core::remote_hosts`](crate::runtime::Core::remote_hosts),
//! which keeps the tokens where the core keeps its secret values and closes the
//! connections when the core shuts down; it can also be made by itself):
//!
//! ```ignore
//! let hosts = core.remote_hosts();                // or RemoteHosts::new(store, TokenVault::keyring())
//! // pairing: from a QR code / deep link or from what the person typed
//! let target = parse_pairing_uri(&text)?;          // or parse_manual_entry(host, port, code)
//! let host = hosts.add(&target, "my laptop").await?;
//! if let TokenState::MemoryOnly { reason } = &host.token { /* tell the user */ }
//! hosts.connect(&host.record.server_id).await?;
//! // commands and events of that host, as JSON
//! let pong = hosts.call(&host.record.server_id, &ApiCommand::Ping).await?;
//! let mut notices = hosts.subscribe();             // HostNotice::{Event, Status, HostsChanged}
//! ```
//!
//! [`RemoteHosts::subscribe`] is one stream for every host: the host's events
//! (as the host sent them) and the changes of each connection's state. Like the
//! core's own events it can lag; a reader that lagged takes the hosts' state
//! again ([`RemoteHosts::list`], and a snapshot from the host).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use super::address::HostPolicy;
use super::connection::{
    CallError, ConnectionConfig, ConnectionState, EventPayload, RemoteConnection,
};
use super::credentials::{TokenPlace, TokenVault};
use super::pair::{PairedHost, PairingFailure, pair};
use super::target::PairingTarget;
use super::transport::TungsteniteTransport;
use super::version::{AppVersion, Compatibility, compatibility};
use crate::api::ApiCommand;
use crate::clock::now_ms;
use crate::runtime::APP_VERSION;
use crate::store::Store;

/// The key of the host list in the database's app settings.
pub const HOSTS_KEY: &str = "remote_client.hosts";
/// Notices a slow reader may fall behind by.
const NOTICE_BUFFER: usize = 1024;
/// How long the `ping` that follows each connection may take.
const PING_TIMEOUT: Duration = Duration::from_secs(10);

/// A host that was paired with, as the database keeps it (nothing secret).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteHostRecord {
    /// Identifies the host's Yhtye (what pairing returned as `server_id`).
    pub server_id: String,
    /// What to call the host: the name of its machine once it has told (after
    /// the first connection), until then the address it was paired by.
    pub name: String,
    /// Where to connect (Tailscale addresses), in the order to try.
    pub hosts: Vec<String>,
    pub port: u16,
    /// This device's id on the host.
    pub device_id: String,
    /// The name this device is listed under on the host.
    pub device_name: String,
    pub added_ms: u64,
    /// The token was put in the OS credential store (not only in memory).
    #[serde(default)]
    pub token_in_keyring: bool,
}

/// What is known of the token of a host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "place", rename_all = "snake_case")]
pub enum TokenState {
    /// In the OS credential store.
    Keyring,
    /// Only in the memory of this process, because the credential store failed
    /// (`reason`): after a restart it is gone and the host must be paired again.
    MemoryOnly { reason: String },
    /// Not anywhere any more (it was memory only, and the app was restarted).
    /// Pair again.
    Missing,
}

/// What the host said about itself (the answer to the `ping` after connecting).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerInfo {
    /// The host's machine name.
    pub host: String,
    /// The host's Yhtye version; none if it predates the field.
    pub version: Option<String>,
    /// The host runs without a GUI (`yhtye --headless`, `core-design.md` §9);
    /// a host that predates the field is taken for one with a GUI.
    pub headless: bool,
    pub compatibility: Compatibility,
}

/// The live state of a host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostStatus {
    pub connection: ConnectionState,
    /// Known once connected (kept while reconnecting).
    pub server: Option<ServerInfo>,
}

impl HostStatus {
    fn idle() -> Self {
        Self {
            connection: ConnectionState::Idle,
            server: None,
        }
    }
}

/// A host with its live state, for a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteHostInfo {
    #[serde(flatten)]
    pub record: RemoteHostRecord,
    pub token: TokenState,
    pub status: HostStatus,
}

/// What [`RemoteHosts::subscribe`] reports.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostNotice {
    /// The list changed (a host was added, removed or renamed): read it again.
    HostsChanged,
    /// A host's connection or server info changed.
    Status {
        server_id: String,
        status: HostStatus,
    },
    /// An event of a host, as it sent it.
    Event {
        server_id: String,
        event: EventPayload,
    },
}

/// What [`RemoteHosts::remove`] could not undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveReport {
    /// The credential store would not delete the token: a copy may be left there.
    pub token_left_in_keyring: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum HostsError {
    #[error("unknown host {0}")]
    UnknownHost(String),
    #[error(transparent)]
    Pairing(#[from] PairingFailure),
    /// A host that is not a Tailscale address (checked again when pairing and
    /// connecting, whatever made the target or the stored record).
    #[error("{0} is not a Tailscale address")]
    NotTailscaleHost(String),
    /// The token is gone (it was kept in memory only): pair again.
    #[error("the device token of this host is gone (it was only kept in memory); pair again")]
    TokenMissing,
    #[error("the OS credential store could not be read: {0}")]
    Credentials(String),
    #[error("the host list could not be stored: {0}")]
    Store(String),
    #[error("the stored host list is unreadable: {0}")]
    Corrupt(String),
    /// [`RemoteHosts::connect`] was not called for this host (or it was disconnected).
    #[error("not connected to host {0}")]
    NotConnected(String),
    #[error(transparent)]
    Call(#[from] CallError),
}

struct Live {
    connection: RemoteConnection,
    status: Arc<Mutex<HostStatus>>,
    /// Stops the tasks that follow the connection.
    watchers: CancellationToken,
}

struct Inner {
    store: Store,
    vault: TokenVault,
    config: ConnectionConfig,
    policy: HostPolicy,
    app_version: Option<AppVersion>,
    notices: broadcast::Sender<HostNotice>,
    /// Serializes the changes: pairing, removing, connecting, disconnecting.
    changes: tokio::sync::Mutex<()>,
    live: Mutex<HashMap<String, Live>>,
}

/// The paired hosts and their connections. Cheap to clone.
#[derive(Clone)]
pub struct RemoteHosts {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for RemoteHosts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteHosts").finish_non_exhaustive()
    }
}

impl RemoteHosts {
    /// Hosts kept in `store` (the local core's database), tokens in `vault`.
    #[must_use]
    pub fn new(store: Store, vault: TokenVault) -> Self {
        Self::with_options(
            store,
            vault,
            ConnectionConfig::default(),
            HostPolicy::TailscaleOnly,
        )
    }

    /// [`new`](Self::new) with other timings for the connections and, for tests
    /// that run a host on loopback, another [`HostPolicy`].
    #[must_use]
    pub fn with_options(
        store: Store,
        vault: TokenVault,
        config: ConnectionConfig,
        policy: HostPolicy,
    ) -> Self {
        let (notices, _) = broadcast::channel(NOTICE_BUFFER);
        Self {
            inner: Arc::new(Inner {
                store,
                vault,
                config,
                policy,
                app_version: AppVersion::parse(APP_VERSION),
                notices,
                changes: tokio::sync::Mutex::new(()),
                live: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// The events and state changes of every host (see the module docs).
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<HostNotice> {
        self.inner.notices.subscribe()
    }

    /// The hosts, oldest first, with their live state.
    pub async fn list(&self) -> Result<Vec<RemoteHostInfo>, HostsError> {
        let records = self.inner.read_records().await?;
        Ok(records.into_iter().map(|r| self.inner.info(r)).collect())
    }

    /// Pairs with the host `target` names, as a desktop client listed under
    /// `device_name`, and remembers it. Does not connect ([`connect`](Self::connect)).
    /// A host already paired with is replaced (the old device stays listed on
    /// that host until it is removed there).
    pub async fn add(
        &self,
        target: &PairingTarget,
        device_name: &str,
    ) -> Result<RemoteHostInfo, HostsError> {
        self.inner.check_hosts(target.hosts())?;
        let paired = pair(target, device_name).await?;
        let _changes = self.inner.changes.lock().await;
        self.inner.disconnect_live(&paired.server_id).await;
        let place = self
            .inner
            .vault
            .save(&paired.server_id, &paired.token)
            .await;
        let record = record_of(&paired, matches!(place, TokenPlace::Keyring));
        let mut records = self.inner.read_records().await?;
        match records.iter_mut().find(|r| r.server_id == record.server_id) {
            Some(existing) => *existing = record.clone(),
            None => records.push(record.clone()),
        }
        if let Err(e) = self.inner.write_records(&records).await {
            // Without a record the token is useless; do not leave it around.
            let _ = self.inner.vault.forget(&record.server_id).await;
            return Err(e);
        }
        self.inner.notify(HostNotice::HostsChanged);
        Ok(self.inner.info(record))
    }

    /// Forgets a host: disconnects, drops the token and the record. The host
    /// still lists this device until it is removed there (this app cannot).
    pub async fn remove(&self, server_id: &str) -> Result<RemoveReport, HostsError> {
        let _changes = self.inner.changes.lock().await;
        let mut records = self.inner.read_records().await?;
        let Some(at) = records.iter().position(|r| r.server_id == server_id) else {
            return Err(HostsError::UnknownHost(server_id.to_string()));
        };
        self.inner.disconnect_live(server_id).await;
        records.remove(at);
        self.inner.write_records(&records).await?;
        let left = self
            .inner
            .vault
            .forget(server_id)
            .await
            .err()
            .map(|e| e.to_string());
        self.inner.notify(HostNotice::HostsChanged);
        Ok(RemoveReport {
            token_left_in_keyring: left,
        })
    }

    /// Connects to the host and keeps the connection up (reconnecting) until
    /// [`disconnect`](Self::disconnect). Already connecting: tries at once.
    /// Progress is in the [`HostNotice::Status`] notices.
    pub async fn connect(&self, server_id: &str) -> Result<(), HostsError> {
        let _changes = self.inner.changes.lock().await;
        let record = self.inner.record(server_id).await?;
        if let Some(connection) = self.inner.live_connection(server_id) {
            connection.start().await;
            return Ok(());
        }
        let token = match self.inner.vault.load(server_id).await {
            Ok(Some(token)) => token,
            Ok(None) => return Err(HostsError::TokenMissing),
            Err(e) => return Err(HostsError::Credentials(e.to_string())),
        };
        self.inner.check_hosts(&record.hosts)?;
        let transport = TungsteniteTransport::new(record.hosts, record.port, token);
        let connection = RemoteConnection::new(Arc::new(transport), self.inner.config.clone());
        let status = Arc::new(Mutex::new(HostStatus::idle()));
        let watchers = CancellationToken::new();
        // Subscribed before it starts, so no event is missed.
        let events = connection.subscribe_events();
        self.inner.lock_live().insert(
            server_id.to_string(),
            Live {
                connection: connection.clone(),
                status: Arc::clone(&status),
                watchers: watchers.clone(),
            },
        );
        tokio::spawn(forward_events(
            Arc::clone(&self.inner),
            server_id.to_string(),
            events,
            watchers.clone(),
        ));
        tokio::spawn(follow_state(
            Arc::clone(&self.inner),
            server_id.to_string(),
            connection.clone(),
            status,
            watchers,
        ));
        connection.start().await;
        Ok(())
    }

    /// Closes the connection of a host (it stays in the list).
    pub async fn disconnect(&self, server_id: &str) -> Result<(), HostsError> {
        let _changes = self.inner.changes.lock().await;
        self.inner.record(server_id).await?;
        self.inner.disconnect_live(server_id).await;
        Ok(())
    }

    /// Runs `command` on the host and returns the reply's `ok` value (JSON, with
    /// a `type`). The host must be connected ([`connect`](Self::connect)); a
    /// command given while it is still connecting waits for it.
    pub async fn call(&self, server_id: &str, command: &ApiCommand) -> Result<Value, HostsError> {
        let connection = self.connection(server_id)?;
        Ok(connection.call(command).await?)
    }

    /// [`call`](Self::call) with its own timeout.
    pub async fn call_with_timeout(
        &self,
        server_id: &str,
        command: &ApiCommand,
        timeout: Duration,
    ) -> Result<Value, HostsError> {
        let connection = self.connection(server_id)?;
        Ok(connection.call_with_timeout(command, timeout).await?)
    }

    /// Closes every connection (the app is quitting). The list stays.
    pub async fn shutdown(&self) {
        let _changes = self.inner.changes.lock().await;
        let all: Vec<String> = self.inner.lock_live().keys().cloned().collect();
        for server_id in all {
            self.inner.disconnect_live(&server_id).await;
        }
    }

    fn connection(&self, server_id: &str) -> Result<RemoteConnection, HostsError> {
        self.inner
            .live_connection(server_id)
            .ok_or_else(|| HostsError::NotConnected(server_id.to_string()))
    }
}

fn record_of(paired: &PairedHost, token_in_keyring: bool) -> RemoteHostRecord {
    RemoteHostRecord {
        server_id: paired.server_id.clone(),
        name: paired.hosts.first().cloned().unwrap_or_default(),
        hosts: paired.hosts.clone(),
        port: paired.port,
        device_id: paired.device_id.clone(),
        device_name: paired.device_name.clone(),
        added_ms: now_ms(),
        token_in_keyring,
    }
}

impl Inner {
    fn lock_live(&self) -> MutexGuard<'_, HashMap<String, Live>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn live_connection(&self, server_id: &str) -> Option<RemoteConnection> {
        self.lock_live()
            .get(server_id)
            .map(|l| l.connection.clone())
    }

    /// `Err` for a host the policy does not allow.
    fn check_hosts(&self, hosts: &[String]) -> Result<(), HostsError> {
        match hosts.iter().find(|h| !self.policy.allows(h)) {
            Some(bad) => Err(HostsError::NotTailscaleHost(bad.clone())),
            None => Ok(()),
        }
    }

    fn notify(&self, notice: HostNotice) {
        // Nobody listening is fine.
        let _ = self.notices.send(notice);
    }

    async fn read_records(&self) -> Result<Vec<RemoteHostRecord>, HostsError> {
        let text = self
            .store
            .app_setting(HOSTS_KEY)
            .await
            .map_err(|e| HostsError::Store(e.to_string()))?;
        match text {
            None => Ok(Vec::new()),
            Some(text) => {
                serde_json::from_str(&text).map_err(|e| HostsError::Corrupt(e.to_string()))
            }
        }
    }

    async fn write_records(&self, records: &[RemoteHostRecord]) -> Result<(), HostsError> {
        let text = serde_json::to_string(records).map_err(|e| HostsError::Store(e.to_string()))?;
        self.store
            .set_app_setting(HOSTS_KEY, &text, now_ms())
            .await
            .map_err(|e| HostsError::Store(e.to_string()))
    }

    async fn record(&self, server_id: &str) -> Result<RemoteHostRecord, HostsError> {
        self.read_records()
            .await?
            .into_iter()
            .find(|r| r.server_id == server_id)
            .ok_or_else(|| HostsError::UnknownHost(server_id.to_string()))
    }

    fn info(&self, record: RemoteHostRecord) -> RemoteHostInfo {
        let token = match self.vault.memory_only_reason(&record.server_id) {
            Some(reason) => TokenState::MemoryOnly { reason },
            None if record.token_in_keyring => TokenState::Keyring,
            None => TokenState::Missing,
        };
        let status = self
            .lock_live()
            .get(&record.server_id)
            .map_or_else(HostStatus::idle, |l| {
                l.status
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
            });
        RemoteHostInfo {
            record,
            token,
            status,
        }
    }

    /// Stops the connection of a host and the tasks that follow it.
    async fn disconnect_live(&self, server_id: &str) {
        let live = self.lock_live().remove(server_id);
        let Some(live) = live else {
            return;
        };
        live.watchers.cancel();
        live.connection.stop().await;
        self.notify(HostNotice::Status {
            server_id: server_id.to_string(),
            status: HostStatus::idle(),
        });
    }

    /// Changes a host's status and tells everyone; nothing if that connection
    /// is no longer the host's (a task that outlived its disconnection). Done
    /// under the lock of the live connections, so that a status never follows
    /// the "idle" notice of the disconnection.
    fn update_status(
        &self,
        server_id: &str,
        status: &Arc<Mutex<HostStatus>>,
        change: impl FnOnce(&mut HostStatus),
    ) {
        let live = self.lock_live();
        if !live
            .get(server_id)
            .is_some_and(|l| Arc::ptr_eq(&l.status, status))
        {
            return;
        }
        let snapshot = {
            let mut status = status.lock().unwrap_or_else(PoisonError::into_inner);
            change(&mut status);
            status.clone()
        };
        self.notify(HostNotice::Status {
            server_id: server_id.to_string(),
            status: snapshot,
        });
    }

    /// The host told the name of its machine: that is its name now.
    async fn learn_name(&self, server_id: &str, name: &str) {
        let _changes = self.changes.lock().await;
        let Ok(mut records) = self.read_records().await else {
            return;
        };
        let Some(record) = records.iter_mut().find(|r| r.server_id == server_id) else {
            return;
        };
        if record.name == name {
            return;
        }
        record.name = name.to_string();
        match self.write_records(&records).await {
            Ok(()) => self.notify(HostNotice::HostsChanged),
            Err(e) => tracing::warn!("could not store the name of host {server_id}: {e}"),
        }
    }
}

/// Passes a host's events on to [`RemoteHosts::subscribe`].
async fn forward_events(
    inner: Arc<Inner>,
    server_id: String,
    mut events: broadcast::Receiver<EventPayload>,
    stop: CancellationToken,
) {
    loop {
        let event = tokio::select! {
            () = stop.cancelled() => return,
            event = events.recv() => event,
        };
        match event {
            Ok(event) => inner.notify(HostNotice::Event {
                server_id: server_id.clone(),
                event,
            }),
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!("{missed} events of host {server_id} were missed");
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

/// Mirrors a connection's state into the host's status, and asks the host
/// who it is each time it connects.
async fn follow_state(
    inner: Arc<Inner>,
    server_id: String,
    connection: RemoteConnection,
    status: Arc<Mutex<HostStatus>>,
    stop: CancellationToken,
) {
    let mut state = connection.watch_state();
    let mut introduced = None;
    loop {
        let current = state.borrow_and_update().clone();
        let session = match &current {
            ConnectionState::Connected { session } => Some(*session),
            _ => None,
        };
        inner.update_status(&server_id, &status, |s| s.connection = current);
        if session.is_some() && session != introduced {
            introduced = session;
            tokio::spawn(introduce(
                Arc::clone(&inner),
                server_id.clone(),
                connection.clone(),
                Arc::clone(&status),
            ));
        }
        tokio::select! {
            () = stop.cancelled() => return,
            changed = state.changed() => if changed.is_err() { return },
        }
    }
}

/// `ping`s the host after a connection: its machine name and version, and
/// whether that version works with this app.
async fn introduce(
    inner: Arc<Inner>,
    server_id: String,
    connection: RemoteConnection,
    status: Arc<Mutex<HostStatus>>,
) {
    let reply = match connection
        .call_with_timeout(&ApiCommand::Ping, PING_TIMEOUT)
        .await
    {
        Ok(reply) => reply,
        Err(e) => {
            tracing::debug!("host {server_id} did not answer the first ping: {e}");
            return;
        }
    };
    let Some(Pong {
        host,
        version,
        headless,
    }) = pong(&reply)
    else {
        tracing::warn!("host {server_id} answered the ping with something else: {reply}");
        return;
    };
    let compatible = match &inner.app_version {
        Some(app) => compatibility(version.as_deref(), app),
        None => Compatibility::Unknown,
    };
    let info = ServerInfo {
        host: host.clone(),
        version,
        headless,
        compatibility: compatible,
    };
    inner.update_status(&server_id, &status, |s| s.server = Some(info));
    inner.learn_name(&server_id, &host).await;
}

/// What a `pong` says.
#[derive(Debug, PartialEq, Eq)]
struct Pong {
    host: String,
    /// `None`: empty or missing (a host that predates the field).
    version: Option<String>,
    /// Missing means a host with a GUI.
    headless: bool,
}

fn pong(reply: &Value) -> Option<Pong> {
    if reply.get("type")? != "pong" {
        return None;
    }
    let host = reply.get("host")?.as_str()?.to_string();
    let version = reply
        .get("version")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .map(str::to_string);
    let headless = reply
        .get("headless")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(Pong {
        host,
        version,
        headless,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiResponse;

    fn pong_of(host: &str, version: Option<&str>, headless: bool) -> Option<Pong> {
        Some(Pong {
            host: host.into(),
            version: version.map(str::to_string),
            headless,
        })
    }

    #[test]
    fn a_pong_is_read_with_or_without_a_version_and_headless_flag() {
        let real = serde_json::to_value(ApiResponse::Pong {
            host: "pc".into(),
            version: "0.1.0".into(),
            headless: true,
        })
        .unwrap();
        assert_eq!(pong(&real), pong_of("pc", Some("0.1.0"), true));
        // A host that predates a field leaves it out; an empty version is the same.
        let old = serde_json::json!({"type": "pong", "host": "old"});
        assert_eq!(pong(&old), pong_of("old", None, false));
        let empty = serde_json::json!({"type": "pong", "host": "old", "version": ""});
        assert_eq!(pong(&empty), pong_of("old", None, false));
        assert_eq!(
            pong(&serde_json::json!({"type": "projects", "host": "x"})),
            None
        );
    }
}
