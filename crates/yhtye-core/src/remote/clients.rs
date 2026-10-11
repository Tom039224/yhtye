//! The clients of this Yhtye as every client may see them (`core-design.md`
//! §18.8): the server (this PC) and the paired devices, read only. The
//! settings panel's device list ([`super::RemoteDevice`]) stays local only:
//! it is for managing devices, this view is for showing who is there.

use schemars::JsonSchema;
use serde::Serialize;
use ts_rs::TS;

use super::RemoteDevice;

/// What a paired device is (only the Android client exists so far).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    Android,
}

/// The machine the core runs on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct ClientServer {
    /// Its host name (as `pong` tells).
    pub host: String,
}

/// A paired device as every client may see it (no token, no hash, nothing to
/// manage it with).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct ClientDevice {
    pub id: String,
    pub name: String,
    pub kind: ClientKind,
    /// It is connected now.
    pub connected: bool,
    /// When it last connected (`null`: never since pairing).
    pub last_seen_ms: Option<u64>,
}

/// Answer of `list_clients` and content of `clients_changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema, TS)]
pub struct Clients {
    pub server: ClientServer,
    /// The paired devices, oldest first.
    pub devices: Vec<ClientDevice>,
}

impl Clients {
    /// The view of the devices the settings panel lists.
    #[must_use]
    pub fn new(host: String, devices: &[RemoteDevice]) -> Self {
        Self {
            server: ClientServer { host },
            devices: devices
                .iter()
                .map(|d| ClientDevice {
                    id: d.id.clone(),
                    name: d.name.clone(),
                    // Every paired device is the Android client for now.
                    kind: ClientKind::Android,
                    connected: d.connected,
                    last_seen_ms: d.last_seen_ms,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_keeps_what_is_shown_and_drops_the_creation_time() {
        let devices = [RemoteDevice {
            id: "dev-1".into(),
            name: "Pixel".into(),
            created_ms: 5,
            last_seen_ms: Some(9),
            connected: true,
        }];
        let clients = Clients::new("pc".into(), &devices);
        let json = serde_json::to_value(&clients).expect("json");
        assert_eq!(
            json,
            serde_json::json!({
                "server": {"host": "pc"},
                "devices": [{"id": "dev-1", "name": "Pixel", "kind": "android", "connected": true, "last_seen_ms": 9}],
            })
        );
    }
}
