// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import io.github.tom039224.yhtye.shared.api.UntaggedSerializer
import io.github.tom039224.yhtye.shared.api.UntaggedVariant
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

@Serializable(with = ResultOfAnyValueOrToolError.Serializer::class)
sealed interface ResultOfAnyValueOrToolError {
    @Serializable
    data class Ok(
        @SerialName("Ok")
        val ok: JsonElement,
    ) : ResultOfAnyValueOrToolError

    @Serializable
    data class Err(
        @SerialName("Err")
        val err: ToolError,
    ) : ResultOfAnyValueOrToolError

    /** A shape this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(override val raw: JsonElement) : ResultOfAnyValueOrToolError, UnrecognizedVariant

    object Serializer : UntaggedSerializer<ResultOfAnyValueOrToolError>(
        "ResultOfAnyValueOrToolError",
        listOf(
            UntaggedVariant<ResultOfAnyValueOrToolError>(listOf(setOf("Ok")), Ok.serializer()) { it is Ok },
            UntaggedVariant<ResultOfAnyValueOrToolError>(listOf(setOf("Err")), Err.serializer()) { it is Err },
        ),
        ::Unrecognized,
    )
}
