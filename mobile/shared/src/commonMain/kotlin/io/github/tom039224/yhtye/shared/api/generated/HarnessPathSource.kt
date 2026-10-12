// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * Where an executable was found.
 */
@Serializable(with = HarnessPathSource.Serializer::class)
enum class HarnessPathSource(override val wire: String) : WireEnum {
    /**
     * The path the user gave.
     */
    Override("override"),
    /**
     * A directory of `PATH`.
     */
    Path("path"),
    /**
     * One of [`KNOWN_DIRS`].
     */
    KnownDir("known_dir"),
    /**
     * Not found.
     */
    None("none"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<HarnessPathSource>("HarnessPathSource", HarnessPathSource.entries, Unrecognized)
}
