use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use yhtye_core::api::{ApiErrorCode, ApiResponse};
use yhtye_core::remote_client::{
    Backoff, CallError, Compatibility, ConnectionConfig, ConnectionState, HostNotice, HostPolicy,
    HostStatus, ServerInfo, TokenVault, HOSTS_KEY,
};
use yhtye_core::secrets::{MemoryBackend, SecretBackend};
use yhtye_core::store::{db_path, Store};

use super::*;

const URI: &str =
    "yhtye://pair?v=1&host=pc.tail1234.ts.net&ip=100.64.0.7&port=1500&code=abcd-2345&sid=sid-1";

fn json_of<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("serializable")
}

// ---- errors ----------------------------------------------------------------

#[test]
fn a_hosts_own_error_reaches_the_webview_unchanged() {
    let host_error = ApiError::not_found("no such project");
    let e = api_error(HostsError::Call(CallError::Api(host_error.clone())));
    assert_eq!(e, host_error);
}

#[test]
fn failures_of_the_link_are_unavailable_and_those_of_the_input_are_not() {
    let code = |e: HostsError| api_error(e).code;
    assert_eq!(
        code(HostsError::NotConnected("sid".into())),
        ApiErrorCode::Unavailable
    );
    assert_eq!(code(HostsError::TokenMissing), ApiErrorCode::Unavailable);
    assert_eq!(
        code(HostsError::Call(CallError::ConnectionLost)),
        ApiErrorCode::Unavailable
    );
    assert_eq!(
        code(HostsError::Call(CallError::Timeout {
            command: "ping".into(),
            timeout: Duration::from_secs(1)
        })),
        ApiErrorCode::Unavailable
    );
    assert_eq!(
        code(HostsError::Call(CallError::NotConnected(
            ConnectionState::Revoked
        ))),
        ApiErrorCode::Unavailable
    );
    assert_eq!(
        code(HostsError::UnknownHost("sid".into())),
        ApiErrorCode::NotFound
    );
    assert_eq!(
        code(HostsError::NotTailscaleHost("1.2.3.4".into())),
        ApiErrorCode::InvalidArgument
    );
    assert_eq!(
        code(HostsError::Store("disk".into())),
        ApiErrorCode::Internal
    );
    assert_eq!(
        code(HostsError::Call(CallError::Encode("x".into()))),
        ApiErrorCode::Internal
    );
}

#[test]
fn a_refused_pairing_says_why() {
    let code = |f: PairingFailure| api_error(HostsError::Pairing(f)).code;
    assert_eq!(
        code(PairingFailure::CodeRejected(None)),
        ApiErrorCode::Forbidden
    );
    assert_eq!(
        code(PairingFailure::OtherUser(None)),
        ApiErrorCode::Forbidden
    );
    assert_eq!(
        code(PairingFailure::BadRequest(None)),
        ApiErrorCode::InvalidArgument
    );
    assert_eq!(
        code(PairingFailure::WrongServer {
            expected: "a".into(),
            actual: "b".into()
        }),
        ApiErrorCode::Conflict
    );
    assert_eq!(
        code(PairingFailure::Unreachable {
            hosts: vec!["100.64.0.7".into()],
            port: 1423,
            cause: None
        }),
        ApiErrorCode::Unavailable
    );
    let rejected = api_error(HostsError::Pairing(PairingFailure::CodeRejected(Some(
        "expired".into(),
    ))));
    assert!(rejected.message.contains("expired"), "{rejected}");
}

/// The screen words a failure by the start of its English message (`describePairError` in
/// `src/ui/hostLabels.ts`): a change of these starts has to change that function too.
#[test]
fn the_messages_the_screen_tells_apart_start_as_it_expects() {
    let message = |f: PairingFailure| api_error(HostsError::Pairing(f)).message;
    assert!(
        message(PairingFailure::CodeRejected(Some("expired".into())))
            .starts_with("the pairing code was refused: expired")
    );
    assert!(message(PairingFailure::OtherUser(None))
        .starts_with("the host does not take devices of other Tailscale users"));
    assert!(message(PairingFailure::BadRequest(None))
        .starts_with("the host did not accept the request"));
    assert!(message(PairingFailure::WrongServer {
        expected: "a".into(),
        actual: "b".into()
    })
    .starts_with("another Yhtye answered"));
    assert!(message(PairingFailure::Unreachable {
        hosts: vec!["100.64.0.7".into()],
        port: 1423,
        cause: None
    })
    .starts_with("could not reach"));
    assert!(message(PairingFailure::ServerError {
        status: 500,
        message: None
    })
    .starts_with("the host answered with an error"));

    let invalid = |input: Value| {
        let input: PairingInput = serde_json::from_value(input).unwrap();
        input.target(HostPolicy::TailscaleOnly).unwrap_err().message
    };
    let manual = |host: &str, port: &str, code: &str| {
        invalid(json!({"kind": "manual", "host": host, "port": port, "code": code}))
    };
    assert_eq!(
        manual("192.168.1.5", "1423", "ABCD2345"),
        "192.168.1.5 is not a Tailscale address"
    );
    assert!(manual("", "1423", "ABCD2345").starts_with("no host was given"));
    assert!(manual("100.64.0.7", "x", "ABCD2345").starts_with("the port must be a number"));
    assert!(manual("100.64.0.7", "1423", "").starts_with("no pairing code was given"));
    assert!(manual("100.64.0.7", "1423", "0000").starts_with("the pairing code must be"));
    assert!(
        invalid(json!({"kind": "uri", "uri": "https://example.com"}))
            .starts_with("this is not a Yhtye pairing code")
    );
    assert!(
        invalid(json!({"kind": "uri", "uri": "yhtye://pair?v=9&ip=100.64.0.7"}))
            .starts_with("this pairing code is of a newer format")
    );
}

// ---- events ----------------------------------------------------------------

#[test]
fn an_event_carries_whose_it_is() {
    let local = ApiEvent::app_wide(ApiEventBody::Resync);
    let value = json_of(&EventEnvelope {
        host: None,
        event: &local,
    });
    assert_eq!(value["host"], Value::Null);
    assert_eq!(value["event"]["body"]["type"], "resync");
    assert_eq!(value["event"]["project"], "");

    // A remote host's event is passed on as the host sent it.
    let sent = json!({"seq": 7, "ts_ms": 1, "project": "p", "live": false, "body": {"type": "x"}});
    let value = json_of(&EventEnvelope {
        host: Some("sid-1"),
        event: &sent,
    });
    assert_eq!(value["host"], "sid-1");
    assert_eq!(value["event"], sent);
}

#[test]
fn the_host_notices_have_the_shape_the_webview_reads() {
    let status = HostNotice::Status {
        server_id: "sid-1".into(),
        status: HostStatus {
            connection: ConnectionState::WaitingToReconnect {
                attempt: 2,
                delay_ms: 2000,
                reason: Some("refused".into()),
            },
            server: Some(ServerInfo {
                host: "pc".into(),
                version: Some("0.1.0".into()),
                headless: true,
                compatibility: Compatibility::Incompatible {
                    host: "0.2.0".into(),
                    app: "0.1.0".into(),
                },
            }),
        },
    };
    assert_eq!(
        json_of(&status),
        json!({
            "type": "status",
            "server_id": "sid-1",
            "status": {
                "connection": {"state": "waiting_to_reconnect", "attempt": 2, "delay_ms": 2000, "reason": "refused"},
                "server": {
                    "host": "pc", "version": "0.1.0", "headless": true,
                    "compatibility": {"type": "incompatible", "host": "0.2.0", "app": "0.1.0"}
                }
            }
        })
    );
    assert_eq!(
        json_of(&HostNotice::HostsChanged),
        json!({"type": "hosts_changed"})
    );
}

#[test]
fn a_reply_is_what_the_core_or_the_host_answered() {
    let local = crate::CommandReply::Local(Box::new(ApiResponse::Pong {
        host: "pc".into(),
        version: "0.1.0".into(),
        headless: false,
    }));
    assert_eq!(json_of(&local)["type"], "pong");
    let remote =
        crate::CommandReply::Remote(json!({"type": "pong", "host": "far", "version": "0.1.0"}));
    assert_eq!(json_of(&remote)["host"], "far");
}

// ---- pairing input ---------------------------------------------------------

#[test]
fn a_pairing_link_and_typed_input_name_a_target() {
    let input: PairingInput = serde_json::from_value(json!({"kind": "uri", "uri": URI})).unwrap();
    let target = input.target(HostPolicy::TailscaleOnly).expect("target");
    assert_eq!(target.hosts(), ["pc.tail1234.ts.net", "100.64.0.7"]);
    assert_eq!(target.port(), 1500);
    assert_eq!(target.code(), "ABCD2345");
    assert_eq!(target.server_id(), Some("sid-1"));

    let input: PairingInput = serde_json::from_value(
        json!({"kind": "manual", "host": "100.64.0.7", "port": "", "code": "abcd 2345"}),
    )
    .unwrap();
    let target = input.target(HostPolicy::TailscaleOnly).expect("target");
    assert_eq!(target.hosts(), ["100.64.0.7"]);
    assert_eq!(target.port(), 1423, "an empty port is the default");
}

#[test]
fn input_that_is_not_for_a_tailscale_host_or_not_a_code_is_refused_before_anything_is_sent() {
    let refused = |input: Value| {
        let input: PairingInput = serde_json::from_value(input).unwrap();
        input.target(HostPolicy::TailscaleOnly).unwrap_err()
    };
    let e = refused(
        json!({"kind": "manual", "host": "192.168.1.5", "port": "1423", "code": "ABCD2345"}),
    );
    assert_eq!(e.code, ApiErrorCode::InvalidArgument);
    assert!(e.message.contains("192.168.1.5"), "{e}");
    let e =
        refused(json!({"kind": "manual", "host": "100.64.0.7", "port": "1423", "code": "0000"}));
    assert_eq!(e.code, ApiErrorCode::InvalidArgument);
    let e = refused(json!({"kind": "uri", "uri": "https://example.com"}));
    assert_eq!(e.code, ApiErrorCode::InvalidArgument);
    // Another input kind is no input.
    assert!(serde_json::from_value::<PairingInput>(json!({"kind": "qr"})).is_err());
}

#[test]
fn the_preview_shows_the_host_and_never_the_code() {
    let input = PairingInput::Uri { uri: URI.into() };
    let target = input.target(HostPolicy::TailscaleOnly).unwrap();
    let preview = PairingPreview::of(&target, "my laptop".into());
    let value = json_of(&preview);
    assert_eq!(
        value,
        json!({
            "hosts": ["pc.tail1234.ts.net", "100.64.0.7"],
            "port": 1500,
            "server_id": "sid-1",
            "default_device_name": "my laptop"
        })
    );
    assert!(!value.to_string().contains("ABCD2345"));
}

// ---- connecting the hosts paired with before ---------------------------------

fn record(server_id: &str, port: u16) -> Value {
    json!({
        "server_id": server_id, "name": server_id, "hosts": ["127.0.0.1"], "port": port,
        "device_id": "dev-1", "device_name": "PC", "added_ms": 1, "token_in_keyring": true
    })
}

/// A port nothing listens on.
fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("addr").port()
}

async fn wait_until_not_idle(
    hosts: &yhtye_core::remote_client::RemoteHosts,
    id: &str,
) -> ConnectionState {
    for _ in 0..200 {
        let list = hosts.list().await.expect("list");
        let state = list
            .iter()
            .find(|h| h.record.server_id == id)
            .expect("listed")
            .status
            .connection
            .clone();
        if state != ConnectionState::Idle {
            return state;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{id} stayed idle");
}

#[tokio::test]
async fn every_saved_host_is_connected_at_startup_and_one_that_cannot_be_does_not_stop_the_rest() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&db_path(dir.path())).await.expect("store");
    let backend = Arc::new(MemoryBackend::default());
    let port = closed_port();
    let records = json!([record("gone", port), record("kept", port)]);
    store
        .set_app_setting(HOSTS_KEY, &records.to_string(), 1)
        .await
        .expect("records");
    // "gone" lost its token (it was kept in memory only); "kept" has one.
    backend.set("remote-host:kept", "token").expect("token");
    let config = ConnectionConfig {
        backoff: Backoff {
            initial: Duration::from_millis(30),
            max: Duration::from_millis(60),
        },
        connect_timeout: Duration::from_millis(500),
        ..ConnectionConfig::default()
    };
    let hosts = yhtye_core::remote_client::RemoteHosts::with_options(
        store,
        TokenVault::new(backend),
        config,
        HostPolicy::AnyHost,
    );

    connect_saved(&hosts).await;

    // Tried: nothing listens, so it waits to connect again.
    let state = wait_until_not_idle(&hosts, "kept").await;
    assert!(
        matches!(
            state,
            ConnectionState::Connecting { .. } | ConnectionState::WaitingToReconnect { .. }
        ),
        "{state:?}"
    );
    let list = hosts.list().await.expect("list");
    let gone = list.iter().find(|h| h.record.server_id == "gone").unwrap();
    assert_eq!(
        gone.status.connection,
        ConnectionState::Idle,
        "no token, no connection"
    );
    hosts.shutdown().await;
}

#[tokio::test]
async fn nothing_is_connected_when_no_host_was_paired() {
    let dir = tempfile::tempdir().expect("dir");
    let store = Store::open(&db_path(dir.path())).await.expect("store");
    let hosts = yhtye_core::remote_client::RemoteHosts::new(
        store,
        TokenVault::new(Arc::new(MemoryBackend::default())),
    );
    connect_saved(&hosts).await;
    assert!(hosts.list().await.expect("list").is_empty());
}
