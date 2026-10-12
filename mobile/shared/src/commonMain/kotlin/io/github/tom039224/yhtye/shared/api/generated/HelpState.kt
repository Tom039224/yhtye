// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

@Serializable(with = HelpState.Serializer::class)
enum class HelpState(override val wire: String) : WireEnum {
    Open("open"),
    Answered("answered"),
    /**
     * The task ended (cancelled) before the help was answered.
     */
    Closed("closed"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<HelpState>("HelpState", HelpState.entries, Unrecognized)
}
