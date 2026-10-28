// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * How one attachment of a user message was finally delivered (`prompted`).
 */
@Serializable
data class AttachmentDelivered(
    /**
     * The inbox entry (`inbox_queued`) of the user message it belongs to.
     */
    @SerialName("inbox_id")
    val inboxId: Long,
    /**
     * The attachment ([`AttachmentMeta::id`]).
     */
    val id: String,
    val delivery: AttachmentDelivery,
)
