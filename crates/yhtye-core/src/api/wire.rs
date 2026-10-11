//! Messages of the development WebSocket bridge (`core-design.md` §10). The
//! bridge is `crates/yhtye-dev-bridge`; the message types live here so both sides share
//! one generated definition.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{ApiCommand, ApiError, ApiEvent, ApiResponse};

/// Client → bridge: run `cmd`; the reply carries the same `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct WsRequest {
    pub id: u64,
    pub cmd: ApiCommand,
}

/// Reply to a [`WsRequest`].
#[derive(Debug, Clone, Serialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum WsReply {
    Ok { id: u64, ok: ApiResponse },
    Err { id: u64, err: ApiError },
}

/// Bridge → client: a reply or a pushed event.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum WsServerMessage {
    #[schemars(title = "Reply")]
    Reply(WsReply),
    Event {
        event: ApiEvent,
    },
}
