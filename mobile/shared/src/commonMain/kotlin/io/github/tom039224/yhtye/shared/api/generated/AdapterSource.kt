// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * What an adapter version resolved to.
 */
@Serializable(with = AdapterSource.Serializer::class)
enum class AdapterSource(override val wire: String) : WireEnum {
    /**
     * The verified version (chosen, or the fallback of another choice).
     */
    Verified("verified"),
    /**
     * The latest version, as npm answered.
     */
    Latest("latest"),
    /**
     * The version the user typed.
     */
    Pinned("pinned"),
    /**
     * The adapter installed globally.
     */
    Global("global"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<AdapterSource>("AdapterSource", AdapterSource.entries, Unrecognized)
}
