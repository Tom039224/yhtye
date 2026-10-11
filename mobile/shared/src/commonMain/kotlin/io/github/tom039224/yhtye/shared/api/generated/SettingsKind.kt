// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * What changed in the settings ([`ApiEventBody::SettingsChanged`]).
 */
@Serializable(with = SettingsKind.Serializer::class)
enum class SettingsKind(override val wire: String) : WireEnum {
    /**
     * The harness × model settings of a layer (`set_agent_settings`).
     */
    AgentSettings("agent_settings"),
    /**
     * The registered harnesses (a manual path was set, or a detection found
     * other executables).
     */
    Harnesses("harnesses"),
    /**
     * The names of the secret environment variables.
     */
    Secrets("secrets"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<SettingsKind>("SettingsKind", SettingsKind.entries, Unrecognized)
}
