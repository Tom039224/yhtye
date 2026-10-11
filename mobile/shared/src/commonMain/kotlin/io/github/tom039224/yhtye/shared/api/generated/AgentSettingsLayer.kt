// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * One layer (global or a project): a role without settings inherits.
 */
@Serializable
data class AgentSettingsLayer(
    val orchestrator: RoleSettings? = null,
    val implementer: RoleSettings? = null,
    val investigator: RoleSettings? = null,
    val reviewer: RoleSettings? = null,
)
