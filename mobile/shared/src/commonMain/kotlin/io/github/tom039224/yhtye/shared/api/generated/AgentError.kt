// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.TaggedSerializer
import io.github.tom039224.yhtye.shared.api.TaggedVariant
import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

/**
 * Errors surfaced by the ACP client. Messages are meant to be shown to people.
 */
@Serializable(with = AgentError.Serializer::class)
sealed interface AgentError {
    @Serializable
    data class Spawn(
        val command: String,
        val message: String,
    ) : AgentError

    @Serializable
    data class Startup(
        val step: String,
        val message: String,
    ) : AgentError

    @Serializable
    data class Timeout(
        val step: String,
        val millis: Long,
    ) : AgentError

    @Serializable
    data class Unsupported(
        val requested: String,
        val available: String,
    ) : AgentError

    /**
     * The agent cannot be chosen (no harness is installed, a harness has no
     * model): the message says what to do.
     */
    @Serializable
    data class Setup(
        val message: String,
    ) : AgentError

    @Serializable
    data object Busy : AgentError

    @Serializable
    data class Request(
        val method: String,
        val message: String,
    ) : AgentError

    @Serializable
    data object Closed : AgentError

    /** A `code` this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(
        /** The unknown `code`. */
        val tag: String,
        override val raw: JsonObject,
    ) : AgentError, UnrecognizedVariant

    object Serializer : TaggedSerializer<AgentError>(
        "AgentError",
        "code",
        listOf(
            TaggedVariant<AgentError>("spawn", Spawn.serializer()) { it is Spawn },
            TaggedVariant<AgentError>("startup", Startup.serializer()) { it is Startup },
            TaggedVariant<AgentError>("timeout", Timeout.serializer()) { it is Timeout },
            TaggedVariant<AgentError>("unsupported", Unsupported.serializer()) { it is Unsupported },
            TaggedVariant<AgentError>("setup", Setup.serializer()) { it is Setup },
            TaggedVariant<AgentError>("busy", Busy.serializer()) { it is Busy },
            TaggedVariant<AgentError>("request", Request.serializer()) { it is Request },
            TaggedVariant<AgentError>("closed", Closed.serializer()) { it is Closed },
        ),
        ::Unrecognized,
    )
}
