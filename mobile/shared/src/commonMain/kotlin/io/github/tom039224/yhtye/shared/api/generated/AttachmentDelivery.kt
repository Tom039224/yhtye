// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * How an attachment reaches the agent.
 */
@Serializable(with = AttachmentDelivery.Serializer::class)
enum class AttachmentDelivery(override val wire: String) : WireEnum {
    /**
     * As an ACP content block (`image` or an embedded `resource`).
     */
    Native("native"),
    /**
     * Only saved on disk: the prompt names its absolute path.
     */
    Path("path"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<AttachmentDelivery>("AttachmentDelivery", AttachmentDelivery.entries, Unrecognized)
}
