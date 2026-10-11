// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Everything the settings panel shows (`core-design.md` §15.7).
 */
@Serializable
data class AgentSettingsView(
    val harnesses: List<HarnessInfo>,
    /**
     * Used for a role no layer sets.
     */
    val builtin: AgentChoice,
    val global: AgentSettingsLayer,
    /**
     * The project asked for, with its layer.
     */
    val project: String? = null,
    @SerialName("project_layer")
    val projectLayer: AgentSettingsLayer? = null,
    /**
     * In effect for `project` (or globally).
     */
    val effective: AgentSettings,
    /**
     * Version of all agent settings layers: it grows with every change (also
     * across restarts of the core), so a larger one is newer. Passed back as
     * `set_agent_settings`' `expected_revision`.
     */
    val revision: Long,
)
