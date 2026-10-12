// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * The settings in effect for every role.
 */
@Serializable
data class AgentSettings(
    val orchestrator: RoleSettings,
    val implementer: RoleSettings,
    val investigator: RoleSettings,
    val reviewer: RoleSettings,
)
