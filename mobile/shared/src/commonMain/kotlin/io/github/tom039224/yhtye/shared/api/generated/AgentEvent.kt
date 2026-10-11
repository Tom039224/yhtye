// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.TaggedSerializer
import io.github.tom039224.yhtye.shared.api.TaggedVariant
import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

/**
 * Everything an agent session reports, in order.
 *
 * Ordering guarantees: `Ready` (or a startup error from `spawn_agent`) comes first;
 * the `Output`s of a turn precede its `TurnEnded`; `Exited` is always the last event.
 */
@Serializable(with = AgentEvent.Serializer::class)
sealed interface AgentEvent {
    @Serializable
    data class Ready(
        val data: JsonElement,
    ) : AgentEvent

    /**
     * One `session/update`, typed 1:1 (unknown kinds become [`AgentOutput::Unknown`]).
     */
    @Serializable
    data class Output(
        val data: AgentOutput,
    ) : AgentEvent

    @Serializable
    data class PermissionAutoAnswered(
        val data: AgentEvent.PermissionAutoAnswered.Data,
    ) : AgentEvent {
        @Serializable
        data class Data(
            @SerialName("tool_call")
            val toolCall: JsonElement,
            val options: List<JsonElement>,
            /**
             * `None` means the request was answered `Cancelled` (no allow option).
             */
            val chosen: String? = null,
        )
    }

    @Serializable
    data class TurnEnded(
        val data: ResultOfStringOrAgentError,
    ) : AgentEvent

    /**
     * One stderr line of the agent process (for logs, not for the UI).
     */
    @Serializable
    data class Stderr(
        val data: String,
    ) : AgentEvent

    /**
     * The agent process is gone (always the final event).
     */
    @Serializable
    data class Exited(
        val data: AgentEvent.Exited.Data,
    ) : AgentEvent {
        @Serializable
        data class Data(
            val code: Int? = null,
            val signal: Int? = null,
        )
    }

    /** A `type` this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(
        /** The unknown `type`. */
        val tag: String,
        override val raw: JsonObject,
    ) : AgentEvent, UnrecognizedVariant

    object Serializer : TaggedSerializer<AgentEvent>(
        "AgentEvent",
        "type",
        listOf(
            TaggedVariant<AgentEvent>("ready", Ready.serializer()) { it is Ready },
            TaggedVariant<AgentEvent>("output", Output.serializer()) { it is Output },
            TaggedVariant<AgentEvent>("permission_auto_answered", PermissionAutoAnswered.serializer()) { it is PermissionAutoAnswered },
            TaggedVariant<AgentEvent>("turn_ended", TurnEnded.serializer()) { it is TurnEnded },
            TaggedVariant<AgentEvent>("stderr", Stderr.serializer()) { it is Stderr },
            TaggedVariant<AgentEvent>("exited", Exited.serializer()) { it is Exited },
        ),
        ::Unrecognized,
    )
}
