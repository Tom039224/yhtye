// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import io.github.tom039224.yhtye.shared.api.UntaggedSerializer
import io.github.tom039224.yhtye.shared.api.UntaggedVariant
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

/**
 * Reply to a [`WsRequest`].
 */
@Serializable(with = WsReply.Serializer::class)
sealed interface WsReply {
    @Serializable
    data class Ok(
        val id: Long,
        val ok: ApiResponse,
    ) : WsReply

    @Serializable
    data class Err(
        val id: Long,
        val err: ApiError,
    ) : WsReply

    /** A shape this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(override val raw: JsonElement) : WsReply, UnrecognizedVariant

    object Serializer : UntaggedSerializer<WsReply>(
        "WsReply",
        listOf(
            UntaggedVariant<WsReply>(listOf(setOf("id", "ok")), Ok.serializer()) { it is Ok },
            UntaggedVariant<WsReply>(listOf(setOf("err", "id")), Err.serializer()) { it is Err },
        ),
        ::Unrecognized,
    )
}
