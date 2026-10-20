//! The client side of remote access (`core-design.md` §18.9): this app, as a
//! desktop client, uses another Yhtye (another PC, or one started without a
//! window) as its host, over the same pairing and WebSocket as the Android client
//! (`remote`, `docs/planned/android-client.md`). It is a port of the Android
//! app's shared module (`mobile/shared`): the same input rules, the same
//! protocol, the same states.
//!
//! - [`RemoteHosts`]: what an app uses. Pairs, remembers the hosts (database),
//!   connects, runs commands, passes on events.
//! - [`parse_pairing_uri`], [`parse_manual_entry`]: whom to pair with.
//! - [`pair`]: `POST /pair`.
//! - [`TokenVault`]: the device token, in the OS credential store.
//! - [`RemoteConnection`]: the connection to one host ([`ConnectionState`],
//!   requests with ids, events, reconnecting), over a [`WsTransport`].
//! - [`compatibility`]: whether the host's version and this app's fit.
//!
//! There is no dependency on Tauri here. A [`crate::runtime::Core`] hands out its
//! [`RemoteHosts`] ([`crate::runtime::Core::remote_hosts`]; the tokens go where
//! the core keeps its secret values, and its connections close with the core),
//! and the app's shell forwards between it and the UI.

mod address;
mod connection;
mod credentials;
mod hosts;
mod pair;
mod target;
mod transport;
mod version;

pub use address::{HostPolicy, is_tailscale_host, normalize_host, to_half_width, url_host};
pub use connection::{
    Backoff, CallError, ConnectionConfig, ConnectionState, DEFAULT_CALL_TIMEOUT, EventPayload,
    RemoteConnection,
};
pub use credentials::{TokenPlace, TokenVault};
pub use hosts::{
    HOSTS_KEY, HostNotice, HostStatus, HostsError, RemoteHostInfo, RemoteHostRecord, RemoteHosts,
    RemoveReport, ServerInfo, TokenState,
};
pub use pair::{PAIR_ATTEMPT_TIMEOUT, PairedHost, PairingFailure, pair, pair_with_timeout};
pub use target::{
    InvalidPairingInput, PairingTarget, looks_like_pairing_uri, normalize_device_name,
    normalize_pairing_code, parse_manual_entry, parse_manual_entry_with, parse_pairing_uri,
    parse_pairing_uri_with,
};
pub use transport::{Incoming, Refusal, TransportError, TungsteniteTransport, WsLink, WsTransport};
pub use version::{AppVersion, Compatibility, compatibility};
