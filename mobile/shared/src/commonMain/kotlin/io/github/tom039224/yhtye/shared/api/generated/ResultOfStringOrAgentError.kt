// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import io.github.tom039224.yhtye.shared.api.UntaggedSerializer
import io.github.tom039224.yhtye.shared.api.UntaggedVariant
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

@Serializable(with = ResultOfStringOrAgentError.Serializer::class)
sealed interface ResultOfStringOrAgentError {
    @Serializable
    data class Ok(
        @SerialName("Ok")
        val ok: String,
    ) : ResultOfStringOrAgentError

    @Serializable
    data class Err(
        @SerialName("Err")
        val err: AgentError,
    ) : ResultOfStringOrAgentError

    /** A shape this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(override val raw: JsonElement) : ResultOfStringOrAgentError, UnrecognizedVariant

    object Serializer : UntaggedSerializer<ResultOfStringOrAgentError>(
        "ResultOfStringOrAgentError",
        listOf(
            UntaggedVariant<ResultOfStringOrAgentError>(listOf(setOf("Ok")), Ok.serializer()) { it is Ok },
            UntaggedVariant<ResultOfStringOrAgentError>(listOf(setOf("Err")), Err.serializer()) { it is Err },
        ),
        ::Unrecognized,
    )
}
