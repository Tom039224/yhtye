//! Files the user attaches to a message for a chat's orchestrator
//! (`core-design.md` §20).
//!
//! An attachment is first *staged* (`stage_attachment`: the file is saved under
//! the data directory and described by an [`AttachmentMeta`]) and then *sent*
//! with the next user message (`send_user_message`'s `attachments`). How it
//! reaches the agent is decided per attachment ([`AttachmentDelivery`]):
//!
//! - `native`: an image the agent can see (`promptCapabilities.image`) goes as an
//!   ACP image block, a text file the agent accepts as embedded context
//!   (`promptCapabilities.embeddedContext`) as a resource block;
//! - `path`: anything else is only saved; the prompt says where (a
//!   `[yhtye:attachment]` line) and the agent reads the file with its own tools.
//!
//! Staging only predicts the delivery ([`classify`]); the final decision is made
//! when the prompt is sent, with the capabilities of the agent that actually runs
//! ([`prompt`]), and recorded in the `prompted` event.

mod classify;
mod name;
pub mod prompt;
mod store;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use classify::{Classified, classify, delivery_for};
pub use name::{MAX_NAME_BYTES, sanitize_file_name};
pub use store::{AttachmentError, AttachmentStore, attachments_root};

/// What an attachment is, as far as delivery is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    /// PNG, JPEG, GIF or WebP (by its content): an agent may be sent it as an image.
    Image,
    /// UTF-8 text with a text MIME type or a known text extension.
    Text,
    /// Anything else (PDF, archives, other images, binaries).
    File,
}

/// How an attachment reaches the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentDelivery {
    /// As an ACP content block (`image` or an embedded `resource`).
    Native,
    /// Only saved on disk: the prompt names its absolute path.
    Path,
}

/// A saved attachment, without its content (the event log never holds the data).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct AttachmentMeta {
    /// Identifies it within its chat (`stage_attachment` makes it up).
    pub id: String,
    /// The file name as saved (made safe: no path separators or control
    /// characters), shown to the user and given to the agent.
    pub name: String,
    /// The MIME type Yhtye decided on (the client's, corrected by the content).
    pub mime: String,
    /// Size in bytes.
    pub size: u64,
    pub kind: AttachmentKind,
    /// A prediction while it is staged (and in `inbox_queued`); the final
    /// decision is in the `prompted` event's `attachments`.
    pub delivery: AttachmentDelivery,
}

/// What an agent accepts in a prompt (ACP `promptCapabilities`; the audio
/// capability is of no use to attachments).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PromptCaps {
    /// Image blocks.
    pub image: bool,
    /// Embedded resource blocks (`embeddedContext`).
    pub embedded_context: bool,
}

impl PromptCaps {
    /// Accepts everything an attachment can be sent as.
    pub const ALL: Self = Self {
        image: true,
        embedded_context: true,
    };
}

impl From<&agent_client_protocol::schema::v1::PromptCapabilities> for PromptCaps {
    fn from(c: &agent_client_protocol::schema::v1::PromptCapabilities) -> Self {
        Self {
            image: c.image,
            embedded_context: c.embedded_context,
        }
    }
}

/// How one attachment of a user message was finally delivered (`prompted`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct AttachmentDelivered {
    /// The inbox entry (`inbox_queued`) of the user message it belongs to.
    pub inbox_id: u64,
    /// The attachment ([`AttachmentMeta::id`]).
    pub id: String,
    pub delivery: AttachmentDelivery,
}
