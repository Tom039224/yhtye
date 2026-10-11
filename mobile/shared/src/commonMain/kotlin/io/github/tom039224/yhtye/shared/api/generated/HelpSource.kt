// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.TaggedSerializer
import io.github.tom039224.yhtye.shared.api.TaggedVariant
import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

/**
 * Who raised a help.
 */
@Serializable(with = HelpSource.Serializer::class)
sealed interface HelpSource {
    /**
     * The agent of `role` working on the help's step (via the `help` tool).
     */
    @Serializable
    data class Agent(
        val role: Role,
    ) : HelpSource

    /**
     * Yhtye itself (merge conflict, protocol violation, crash, ...).
     */
    @Serializable
    data object Yhtye : HelpSource

    /** A `by` this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(
        /** The unknown `by`. */
        val tag: String,
        override val raw: JsonObject,
    ) : HelpSource, UnrecognizedVariant

    object Serializer : TaggedSerializer<HelpSource>(
        "HelpSource",
        "by",
        listOf(
            TaggedVariant<HelpSource>("agent", Agent.serializer()) { it is Agent },
            TaggedVariant<HelpSource>("yhtye", Yhtye.serializer()) { it is Yhtye },
        ),
        ::Unrecognized,
    )
}
