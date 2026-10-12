// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * How a delivered `message_task` message reached the agent.
 */
@Serializable(with = MessageVia.Serializer::class)
enum class MessageVia(override val wire: String) : WireEnum {
    /**
     * Injected into the running turn (`_session/steering`).
     */
    Steering("steering"),
    /**
     * The running turn was cancelled and the message sent as the next prompt.
     */
    Interrupt("interrupt"),
    /**
     * Sent as a normal prompt: the agent was between turns.
     */
    Prompt("prompt"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<MessageVia>("MessageVia", MessageVia.entries, Unrecognized)
}
