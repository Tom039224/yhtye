// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.TaggedSerializer
import io.github.tom039224.yhtye.shared.api.TaggedVariant
import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

/**
 * A `session/update`, typed. Mirrors `SessionUpdate` of the ACP schema.
 */
@Serializable(with = AgentOutput.Serializer::class)
sealed interface AgentOutput {
    @Serializable
    data class UserMessageChunk(
        val update: JsonElement,
    ) : AgentOutput

    @Serializable
    data class MessageChunk(
        val update: JsonElement,
    ) : AgentOutput

    @Serializable
    data class ThoughtChunk(
        val update: JsonElement,
    ) : AgentOutput

    @Serializable
    data class ToolCall(
        val update: JsonElement,
    ) : AgentOutput

    @Serializable
    data class ToolCallUpdate(
        val update: JsonElement,
    ) : AgentOutput

    @Serializable
    data class Plan(
        val update: JsonElement,
    ) : AgentOutput

    @Serializable
    data class AvailableCommands(
        val update: JsonElement,
    ) : AgentOutput

    @Serializable
    data class ModeChanged(
        val update: JsonElement,
    ) : AgentOutput

    @Serializable
    data class ConfigOptions(
        val update: JsonElement,
    ) : AgentOutput

    @Serializable
    data class SessionInfo(
        val update: JsonElement,
    ) : AgentOutput

    @Serializable
    data class Usage(
        val update: JsonElement,
    ) : AgentOutput

    /**
     * An update kind this build does not know (kept verbatim, never dropped).
     */
    @Serializable
    data class Unknown(
        val update: JsonElement,
    ) : AgentOutput

    /** A `kind` this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(
        /** The unknown `kind`. */
        val tag: String,
        override val raw: JsonObject,
    ) : AgentOutput, UnrecognizedVariant

    object Serializer : TaggedSerializer<AgentOutput>(
        "AgentOutput",
        "kind",
        listOf(
            TaggedVariant<AgentOutput>("user_message_chunk", UserMessageChunk.serializer()) { it is UserMessageChunk },
            TaggedVariant<AgentOutput>("message_chunk", MessageChunk.serializer()) { it is MessageChunk },
            TaggedVariant<AgentOutput>("thought_chunk", ThoughtChunk.serializer()) { it is ThoughtChunk },
            TaggedVariant<AgentOutput>("tool_call", ToolCall.serializer()) { it is ToolCall },
            TaggedVariant<AgentOutput>("tool_call_update", ToolCallUpdate.serializer()) { it is ToolCallUpdate },
            TaggedVariant<AgentOutput>("plan", Plan.serializer()) { it is Plan },
            TaggedVariant<AgentOutput>("available_commands", AvailableCommands.serializer()) { it is AvailableCommands },
            TaggedVariant<AgentOutput>("mode_changed", ModeChanged.serializer()) { it is ModeChanged },
            TaggedVariant<AgentOutput>("config_options", ConfigOptions.serializer()) { it is ConfigOptions },
            TaggedVariant<AgentOutput>("session_info", SessionInfo.serializer()) { it is SessionInfo },
            TaggedVariant<AgentOutput>("usage", Usage.serializer()) { it is Usage },
            TaggedVariant<AgentOutput>("unknown", Unknown.serializer()) { it is Unknown },
        ),
        ::Unrecognized,
    )
}
