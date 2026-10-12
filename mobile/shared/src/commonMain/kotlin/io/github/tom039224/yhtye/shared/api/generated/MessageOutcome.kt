// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * Whether a `message_task` message reached the agent.
 */
@Serializable(with = MessageOutcome.Serializer::class)
enum class MessageOutcome(override val wire: String) : WireEnum {
    Delivered("delivered"),
    /**
     * Not delivered; the orchestrator decides what to do instead.
     */
    Returned("returned"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<MessageOutcome>("MessageOutcome", MessageOutcome.entries, Unrecognized)
}
