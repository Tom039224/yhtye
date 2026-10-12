// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import io.github.tom039224.yhtye.shared.api.UntaggedSerializer
import io.github.tom039224.yhtye.shared.api.UntaggedVariant
import kotlin.jvm.JvmInline
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

/**
 * Bridge → client: a reply or a pushed event.
 */
@Serializable(with = WsServerMessage.Serializer::class)
sealed interface WsServerMessage {
    @Serializable
    @JvmInline
    value class Reply(val value: WsReply) : WsServerMessage

    @Serializable
    data class Event(
        val event: ApiEvent,
    ) : WsServerMessage

    /** A shape this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(override val raw: JsonElement) : WsServerMessage, UnrecognizedVariant

    object Serializer : UntaggedSerializer<WsServerMessage>(
        "WsServerMessage",
        listOf(
            UntaggedVariant<WsServerMessage>(listOf(setOf("id", "ok"), setOf("err", "id")), Reply.serializer()) { it is Reply },
            UntaggedVariant<WsServerMessage>(listOf(setOf("event")), Event.serializer()) { it is Event },
        ),
        ::Unrecognized,
    )
}
