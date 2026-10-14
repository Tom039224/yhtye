//! The desktop client of remote access (`yhtye_core::remote_client`) against a
//! real remote server with a fake Tailscale (127.0.0.1): pairing as a desktop,
//! connecting, commands, events, revocation, restarts, and what is kept where.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::broadcast;
use yhtye_core::api::{ApiCommand, ApiErrorCode, ApiResponse};
use yhtye_core::remote::{ClientKind, PairingCode, Tailscale};
use yhtye_core::remote_client::{
    Backoff, CallError, Compatibility, ConnectionConfig, ConnectionState, HOSTS_KEY, HostNotice,
    HostPolicy, HostStatus, HostsError, PairingFailure, RemoteHostInfo, RemoteHosts, TokenState,
    TokenVault, pair_with_timeout, parse_manual_entry_with, parse_pairing_uri_with,
};
use yhtye_core::runtime::{APP_VERSION, Core, CoreConfig, host_name};
use yhtye_core::secrets::{MemoryBackend, SecretBackend};
use yhtye_core::store::{Store, db_path};

mod common;

use common::fake_tailscale::{FakeTailscale, ME, SOMEONE_ELSE, free_port};

const TIMEOUT: Duration = Duration::from_secs(15);

/// A host: a core with remote access on, on a free loopback port.
struct Host {
    core: Core,
    port: u16,
    server_id: String,
    _dir: tempfile::TempDir,
}

async fn start_host(tailscale: Arc<dyn Tailscale>) -> Host {
    let dir = tempfile::tempdir().expect("dir");
    let mut cfg = CoreConfig::claude_code(dir.path(), "haiku");
    cfg.usage = None;
    cfg.secret_backend = Arc::new(MemoryBackend::default());
    cfg.tailscale = tailscale;
    let core = Core::start(cfg).await.expect("core");
    let port = free_port();
    let status = match core
        .command(ApiCommand::SetRemoteSettings {
            enabled: true,
            port: Some(port),
        })
        .await
        .expect("enable")
    {
        ApiResponse::RemoteStatus { status } => status,
        other => panic!("unexpected {other:?}"),
    };
    assert!(status.running, "{status:?}");
    Host {
        core,
        port,
        server_id: status.server_id,
        _dir: dir,
    }
}

impl Host {
    async fn code(&self) -> PairingCode {
        match self
            .core
            .command(ApiCommand::CreatePairingCode)
            .await
            .expect("code")
        {
            ApiResponse::PairingCode { pairing } => pairing,
            other => panic!("unexpected {other:?}"),
        }
    }

    /// The target of a fresh pairing code, as typed by hand (the PC's QR code
    /// also names `pc.test.ts.net`, which does not exist here).
    async fn target(&self) -> yhtye_core::remote_client::PairingTarget {
        let code = self.code().await.code;
        parse_manual_entry_with(
            "127.0.0.1",
            &self.port.to_string(),
            &code,
            HostPolicy::AnyHost,
        )
        .expect("target")
    }

    async fn devices(&self) -> Vec<yhtye_core::remote::RemoteDevice> {
        match self
            .core
            .command(ApiCommand::ListRemoteDevices)
            .await
            .expect("devices")
        {
            ApiResponse::RemoteDevices { devices } => devices,
            other => panic!("unexpected {other:?}"),
        }
    }

    async fn set_remote(&self, enabled: bool) {
        self.core
            .command(ApiCommand::SetRemoteSettings {
                enabled,
                port: Some(self.port),
            })
            .await
            .expect("settings");
    }
}

/// Quick reconnects, so that tests do not wait seconds.
fn fast() -> ConnectionConfig {
    ConnectionConfig {
        backoff: Backoff {
            initial: Duration::from_millis(40),
            max: Duration::from_millis(200),
        },
        ..ConnectionConfig::default()
    }
}

/// The local side: its own database and a token vault.
struct Client {
    hosts: RemoteHosts,
    backend: Arc<MemoryBackend>,
    dir: tempfile::TempDir,
}

async fn start_client() -> Client {
    let dir = tempfile::tempdir().expect("dir");
    let backend = Arc::new(MemoryBackend::default());
    let (hosts, _store) = open_client(dir.path(), &backend).await;
    Client {
        hosts,
        backend,
        dir,
    }
}

async fn open_client(dir: &Path, backend: &Arc<MemoryBackend>) -> (RemoteHosts, Store) {
    let store = Store::open(&db_path(dir)).await.expect("store");
    let vault = TokenVault::new(backend.clone() as Arc<dyn SecretBackend>);
    (
        RemoteHosts::with_options(store.clone(), vault, fast(), HostPolicy::AnyHost),
        store,
    )
}

/// Reads notices until one that `pick` takes.
async fn notice<T>(
    notices: &mut broadcast::Receiver<HostNotice>,
    mut pick: impl FnMut(&HostNotice) -> Option<T>,
) -> T {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            match notices.recv().await {
                Ok(n) => {
                    if let Some(found) = pick(&n) {
                        return found;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("notices ended: {e}"),
            }
        }
    })
    .await
    .expect("the notice in time")
}

/// The next status of the host that `pick` takes.
async fn status_where(
    notices: &mut broadcast::Receiver<HostNotice>,
    server_id: &str,
    pick: impl Fn(&HostStatus) -> bool,
) -> HostStatus {
    notice(notices, |n| match n {
        HostNotice::Status {
            server_id: id,
            status,
        } if id == server_id && pick(status) => Some(status.clone()),
        _ => None,
    })
    .await
}

fn connected(s: &HostStatus) -> bool {
    matches!(s.connection, ConnectionState::Connected { .. })
}

async fn only_host(client: &Client) -> RemoteHostInfo {
    let mut listed = client.hosts.list().await.expect("list");
    assert_eq!(listed.len(), 1, "{listed:?}");
    listed.remove(0)
}

#[tokio::test]
async fn pairs_connects_calls_hears_events_and_is_revoked() {
    let host = start_host(FakeTailscale::new()).await;
    let client = start_client().await;
    let mut notices = client.hosts.subscribe();

    // Pairing: the host lists this client as a desktop.
    let added = client
        .hosts
        .add(&host.target().await, "  Workstation ")
        .await
        .expect("paired");
    assert_eq!(added.record.server_id, host.server_id);
    assert_eq!(added.record.hosts, ["127.0.0.1"]);
    assert_eq!(added.record.port, host.port);
    assert_eq!(added.record.device_name, "Workstation");
    assert_eq!(added.token, TokenState::Keyring);
    assert_eq!(added.status.connection, ConnectionState::Idle);
    let devices = host.devices().await;
    assert_eq!(devices.len(), 1);
    assert_eq!(
        (devices[0].name.as_str(), devices[0].kind),
        ("Workstation", ClientKind::Desktop)
    );
    assert_eq!(devices[0].id, added.record.device_id);
    assert!(!devices[0].connected);

    // The token is in the credential store and nowhere in the local database.
    let account = format!("remote-host:{}", host.server_id);
    let token = client
        .backend
        .get(&account)
        .unwrap()
        .expect("a stored token");
    assert_eq!(token.len(), 64);
    for entry in std::fs::read_dir(client.dir.path()).expect("data dir") {
        let path = entry.expect("entry").path();
        if path.is_file() {
            let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
            assert!(!text.contains(&token), "{} holds the token", path.display());
        }
    }
    let listed = serde_json::to_string(&client.hosts.list().await.unwrap()).unwrap();
    assert!(!listed.contains(&token), "the list shows the token");

    // Connecting: the host says who it is, and the versions fit.
    client
        .hosts
        .connect(&host.server_id)
        .await
        .expect("connect");
    let known = status_where(&mut notices, &host.server_id, |s| {
        connected(s) && s.server.is_some()
    })
    .await;
    let server = known.server.expect("server info");
    assert_eq!(server.host, host_name());
    assert_eq!(server.version.as_deref(), Some(APP_VERSION));
    assert!(
        !server.headless,
        "this host has a GUI (a test core is not headless)"
    );
    assert_eq!(server.compatibility, Compatibility::Compatible);
    // The host's name replaces the address it was paired by (and is kept).
    notice(&mut notices, |n| {
        matches!(n, HostNotice::HostsChanged).then_some(())
    })
    .await;
    assert_eq!(only_host(&client).await.record.name, host_name());
    assert!(host.devices().await[0].connected);

    // Commands run on the host; a local-only one is refused there.
    let pong = client
        .hosts
        .call(&host.server_id, &ApiCommand::Ping)
        .await
        .expect("pong");
    assert_eq!(pong["type"], "pong");
    assert_eq!(pong["host"], host_name());
    let clients = client
        .hosts
        .call(&host.server_id, &ApiCommand::ListClients)
        .await
        .expect("clients");
    let me = clients["clients"]["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == added.record.device_id.as_str())
        .expect("this client is listed");
    assert_eq!(
        (me["kind"].as_str(), me["connected"].as_bool()),
        (Some("desktop"), Some(true))
    );
    let refused = client
        .hosts
        .call(&host.server_id, &ApiCommand::GetRemoteStatus)
        .await
        .unwrap_err();
    assert!(
        matches!(&refused, HostsError::Call(CallError::Api(e)) if e.code == ApiErrorCode::Forbidden),
        "{refused:?}"
    );

    // An event of the host reaches the notices: another device pairs.
    let other = host.target().await;
    let second = yhtye_core::remote_client::pair_with_timeout(&other, "Other PC", TIMEOUT)
        .await
        .expect("second pairing");
    let event: Arc<Value> = notice(&mut notices, |n| match n {
        HostNotice::Event { server_id, event }
            if *server_id == host.server_id && event["body"]["type"] == "clients_changed" =>
        {
            Some(event.clone())
        }
        _ => None,
    })
    .await;
    assert_eq!(event["project"], "");
    assert_eq!(
        event["body"]["clients"]["devices"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    drop(second);

    // Revoked on the host: the client learns it at its next try and stops.
    host.core
        .command(ApiCommand::RevokeRemoteDevice {
            device: added.record.device_id.clone(),
        })
        .await
        .expect("revoke");
    status_where(&mut notices, &host.server_id, |s| {
        s.connection == ConnectionState::Revoked
    })
    .await;
    assert_eq!(
        only_host(&client).await.status.connection,
        ConnectionState::Revoked
    );
    let after = client
        .hosts
        .call(&host.server_id, &ApiCommand::Ping)
        .await
        .unwrap_err();
    assert!(
        matches!(
            after,
            HostsError::Call(CallError::NotConnected(ConnectionState::Revoked))
        ),
        "{after:?}"
    );

    // Forgetting the host drops the token with it.
    let report = client.hosts.remove(&host.server_id).await.expect("removed");
    assert_eq!(report.token_left_in_keyring, None);
    assert!(client.hosts.list().await.unwrap().is_empty());
    assert_eq!(client.backend.get(&account).unwrap(), None);
    assert!(matches!(
        client.hosts.remove(&host.server_id).await,
        Err(HostsError::UnknownHost(_))
    ));
    client.hosts.shutdown().await;
    host.core.shutdown().await;
}

#[tokio::test]
async fn the_connection_comes_back_when_the_host_restarts_remote_access() {
    let host = start_host(FakeTailscale::new()).await;
    let client = start_client().await;
    let mut notices = client.hosts.subscribe();
    client
        .hosts
        .add(&host.target().await, "Workstation")
        .await
        .expect("paired");
    client
        .hosts
        .connect(&host.server_id)
        .await
        .expect("connect");
    let first = status_where(&mut notices, &host.server_id, connected).await;
    assert_eq!(first.connection, ConnectionState::Connected { session: 1 });

    host.set_remote(false).await;
    status_where(&mut notices, &host.server_id, |s| {
        matches!(s.connection, ConnectionState::WaitingToReconnect { .. })
    })
    .await;
    host.set_remote(true).await;
    let again = status_where(&mut notices, &host.server_id, connected).await;
    // A new session: events in between were missed, so a reader starts over.
    assert_eq!(again.connection, ConnectionState::Connected { session: 2 });
    let pong = client
        .hosts
        .call(&host.server_id, &ApiCommand::Ping)
        .await
        .expect("pong");
    assert_eq!(pong["type"], "pong");

    client
        .hosts
        .disconnect(&host.server_id)
        .await
        .expect("disconnect");
    assert_eq!(
        only_host(&client).await.status.connection,
        ConnectionState::Idle
    );
    assert!(matches!(
        client.hosts.call(&host.server_id, &ApiCommand::Ping).await,
        Err(HostsError::NotConnected(_))
    ));
    client.hosts.shutdown().await;
    host.core.shutdown().await;
}

#[tokio::test]
async fn another_tailscale_user_is_refused_at_pairing_and_at_connecting() {
    // At pairing: nothing is stored.
    let strangers = start_host(FakeTailscale::with_peer_user(SOMEONE_ELSE)).await;
    let client = start_client().await;
    let refused = client
        .hosts
        .add(&strangers.target().await, "Workstation")
        .await
        .unwrap_err();
    assert!(
        matches!(
            refused,
            HostsError::Pairing(PairingFailure::OtherUser(Some(_)))
        ),
        "{refused:?}"
    );
    assert!(client.hosts.list().await.unwrap().is_empty());
    strangers.core.shutdown().await;

    // At connecting: a paired device whose user changed is refused (403), not revoked.
    let tailscale = FakeTailscale::new();
    let host = start_host(tailscale.clone()).await;
    let client = start_client().await;
    let mut notices = client.hosts.subscribe();
    client
        .hosts
        .add(&host.target().await, "Workstation")
        .await
        .expect("paired");
    client
        .hosts
        .connect(&host.server_id)
        .await
        .expect("connect");
    status_where(&mut notices, &host.server_id, connected).await;
    tailscale.set_peer_user(SOMEONE_ELSE);
    host.set_remote(false).await; // forgets what whois said
    host.set_remote(true).await;
    let rejected = status_where(&mut notices, &host.server_id, |s| {
        matches!(s.connection, ConnectionState::Rejected { .. })
    })
    .await;
    let ConnectionState::Rejected { message } = rejected.connection else {
        unreachable!()
    };
    assert!(
        message
            .expect("the host's reason")
            .contains("another Tailscale user")
    );
    // It does not try again by itself; asking does.
    tailscale.set_peer_user(ME);
    host.set_remote(false).await;
    host.set_remote(true).await;
    client.hosts.connect(&host.server_id).await.expect("again");
    status_where(&mut notices, &host.server_id, connected).await;
    client.hosts.shutdown().await;
    host.core.shutdown().await;
}

#[tokio::test]
async fn pairing_failures_say_what_is_wrong_and_store_nothing() {
    let host = start_host(FakeTailscale::new()).await;
    let client = start_client().await;
    let good = host.code().await.code;
    let port = host.port.to_string();

    // A wrong code.
    let wrong =
        parse_manual_entry_with("127.0.0.1", &port, "AAAAAAAA", HostPolicy::AnyHost).unwrap();
    assert!(matches!(
        client.hosts.add(&wrong, "PC").await,
        Err(HostsError::Pairing(PairingFailure::CodeRejected(Some(_))))
    ));

    // A QR code of another Yhtye (another server id): the answer is thrown away.
    let uri = format!("yhtye://pair?v=1&ip=127.0.0.1&port={port}&code={good}&sid=0123456789abcdef");
    let other_server = parse_pairing_uri_with(&uri, HostPolicy::AnyHost).unwrap();
    let failure = client.hosts.add(&other_server, "PC").await.unwrap_err();
    match failure {
        HostsError::Pairing(PairingFailure::WrongServer { expected, actual }) => {
            assert_eq!(expected, "0123456789abcdef");
            assert_eq!(actual, host.server_id);
        }
        other => panic!("{other:?}"),
    }
    assert!(client.hosts.list().await.unwrap().is_empty());
    assert_eq!(
        client
            .backend
            .get(&format!("remote-host:{}", host.server_id))
            .unwrap(),
        None,
        "no token of a refused pairing is kept"
    );

    // Nobody listening.
    let nobody = parse_manual_entry_with(
        "127.0.0.1",
        &free_port().to_string(),
        "ABCD2345",
        HostPolicy::AnyHost,
    )
    .unwrap();
    assert!(matches!(
        client.hosts.add(&nobody, "PC").await,
        Err(HostsError::Pairing(PairingFailure::Unreachable { .. }))
    ));
    // A device name the host would refuse is refused before asking.
    let target = host.target().await;
    assert!(matches!(
        client.hosts.add(&target, "   ").await,
        Err(HostsError::Pairing(PairingFailure::BadRequest(_)))
    ));
    host.core.shutdown().await;
}

#[tokio::test]
async fn the_next_host_is_tried_when_the_first_does_not_answer() {
    let host = start_host(FakeTailscale::new()).await;
    let code = host.code().await.code;
    // The QR code names the MagicDNS name first; here it does not resolve.
    let uri = format!(
        "yhtye://pair?v=1&host=gone.invalid&ip=127.0.0.1&port={}&code={code}&sid={}",
        host.port, host.server_id
    );
    let target = parse_pairing_uri_with(&uri, HostPolicy::AnyHost).unwrap();
    assert_eq!(target.hosts(), ["gone.invalid", "127.0.0.1"]);
    let paired = pair_with_timeout(&target, "Workstation", Duration::from_secs(3))
        .await
        .expect("paired through the second host");
    // The one that answered is tried first from now on.
    assert_eq!(paired.hosts, ["127.0.0.1", "gone.invalid"]);
    assert_eq!(paired.server_id, host.server_id);
    host.core.shutdown().await;
}

#[tokio::test]
async fn hosts_and_tokens_survive_a_restart() {
    let host = start_host(FakeTailscale::new()).await;
    let client = start_client().await;
    let added = client
        .hosts
        .add(&host.target().await, "Workstation")
        .await
        .expect("paired");
    client.hosts.shutdown().await;
    drop(client.hosts);

    // The same database and credential store, a new process.
    let (hosts, _store) = open_client(client.dir.path(), &client.backend).await;
    let mut listed = hosts.list().await.expect("list");
    assert_eq!(listed.len(), 1);
    let again = listed.remove(0);
    assert_eq!(again.record, added.record);
    assert_eq!(again.token, TokenState::Keyring);
    let mut notices = hosts.subscribe();
    hosts
        .connect(&host.server_id)
        .await
        .expect("connect with the stored token");
    status_where(&mut notices, &host.server_id, connected).await;
    hosts.shutdown().await;
    host.core.shutdown().await;
}

#[tokio::test]
async fn without_a_credential_store_the_token_is_kept_for_this_run_only() {
    let host = start_host(FakeTailscale::new()).await;
    let dir = tempfile::tempdir().expect("dir");
    let backend = Arc::new(MemoryBackend::default());
    backend.fail_with("the Secret Service is not running");
    let (hosts, _store) = open_client(dir.path(), &backend).await;
    let added = hosts
        .add(&host.target().await, "Workstation")
        .await
        .expect("paired");
    let TokenState::MemoryOnly { reason } = &added.token else {
        panic!("{:?}", added.token)
    };
    assert!(reason.contains("Secret Service"), "{reason}");
    let mut notices = hosts.subscribe();
    hosts
        .connect(&host.server_id)
        .await
        .expect("connect from memory");
    status_where(&mut notices, &host.server_id, connected).await;
    hosts.shutdown().await;
    drop(hosts);

    // A new run: the host is remembered, its token is not.
    let (hosts, _store) = open_client(dir.path(), &backend).await;
    let listed = hosts.list().await.expect("list");
    assert_eq!(listed[0].record.server_id, host.server_id);
    assert_eq!(listed[0].token, TokenState::Missing);
    assert!(matches!(
        hosts.connect(&host.server_id).await,
        Err(HostsError::TokenMissing | HostsError::Credentials(_))
    ));
    // Pairing again replaces the entry (one host, not two).
    let again = hosts
        .add(&host.target().await, "Workstation")
        .await
        .expect("paired again");
    assert_eq!(again.record.server_id, host.server_id);
    assert_eq!(hosts.list().await.unwrap().len(), 1);
    assert_eq!(
        host.devices().await.len(),
        2,
        "the earlier device stays listed on the host"
    );
    hosts.shutdown().await;
    host.core.shutdown().await;
}

#[tokio::test]
async fn only_tailscale_hosts_are_ever_talked_to_unless_the_policy_is_loosened() {
    let host = start_host(FakeTailscale::new()).await;
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&db_path(dir.path())).await.expect("store");
    let backend = Arc::new(MemoryBackend::default());
    let strict = RemoteHosts::new(store.clone(), TokenVault::new(backend.clone()));

    // A target made with the loose policy is refused by a manager that keeps the strict one.
    let target = host.target().await;
    assert!(matches!(
        strict.add(&target, "PC").await,
        Err(HostsError::NotTailscaleHost(h)) if h == "127.0.0.1"
    ));
    assert!(host.devices().await.is_empty(), "nothing was sent to it");

    // A record that names another host (edited by hand) is not connected to, either.
    let record = serde_json::json!([{
        "server_id": "sid", "name": "x", "hosts": ["192.168.1.5"], "port": 1423,
        "device_id": "dev-1", "device_name": "PC", "added_ms": 1, "token_in_keyring": true
    }]);
    store
        .set_app_setting(HOSTS_KEY, &record.to_string(), 1)
        .await
        .expect("record");
    backend.set("remote-host:sid", "token").expect("token");
    assert!(matches!(
        strict.connect("sid").await,
        Err(HostsError::NotTailscaleHost(h)) if h == "192.168.1.5"
    ));
    host.core.shutdown().await;
}

#[tokio::test]
async fn a_core_hands_out_its_remote_hosts_with_the_strict_policy() {
    let dir = tempfile::tempdir().expect("dir");
    let mut cfg = CoreConfig::claude_code(dir.path(), "haiku");
    cfg.usage = None;
    cfg.secret_backend = Arc::new(MemoryBackend::default());
    cfg.tailscale = FakeTailscale::new();
    let core = Core::start(cfg).await.expect("core");
    let hosts = core.remote_hosts();
    assert!(hosts.list().await.expect("list").is_empty());
    // The same handle every time; the strict policy applies (the target is loopback).
    let target =
        parse_manual_entry_with("127.0.0.1", "1423", "ABCD2345", HostPolicy::AnyHost).unwrap();
    assert!(matches!(
        core.remote_hosts().add(&target, "PC").await,
        Err(HostsError::NotTailscaleHost(_))
    ));
    core.shutdown().await;
}
