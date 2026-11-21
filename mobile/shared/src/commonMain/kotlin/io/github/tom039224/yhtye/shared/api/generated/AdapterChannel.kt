// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * Which adapter version a harness runs.
 */
@Serializable(with = AdapterChannel.Serializer::class)
enum class AdapterChannel(override val wire: String) : WireEnum {
    /**
     * The version that was tested against Yhtye.
     */
    Verified("verified"),
    /**
     * The newest version npm has (looked up with `npm view`, remembered for a
     * few hours); the verified one when it cannot be looked up.
     */
    Latest("latest"),
    /**
     * The version in [`HarnessOptions::adapter_version`].
     */
    Pinned("pinned"),
    /**
     * The adapter installed globally (`claude-agent-acp` / `codex-acp` on `PATH`).
     */
    Global("global"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<AdapterChannel>("AdapterChannel", AdapterChannel.entries, Unrecognized)
}
