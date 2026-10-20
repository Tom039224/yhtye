// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * A saved attachment, without its content (the event log never holds the data).
 */
@Serializable
data class AttachmentMeta(
    /**
     * Identifies it within its chat (`stage_attachment` makes it up).
     */
    val id: String,
    /**
     * The file name as saved (made safe: no path separators or control
     * characters), shown to the user and given to the agent.
     */
    val name: String,
    /**
     * The MIME type Yhtye decided on (the client's, corrected by the content).
     */
    val mime: String,
    /**
     * Size in bytes.
     */
    val size: Long,
    val kind: AttachmentKind,
    /**
     * A prediction while it is staged (and in `inbox_queued`); the final
     * decision is in the `prompted` event's `attachments`.
     */
    val delivery: AttachmentDelivery,
)
