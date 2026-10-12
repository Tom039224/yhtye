// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * The candidates of one role and its default (which is one of them).
 */
@Serializable
data class RoleSettings(
    val candidates: List<Candidate>,
    val default: AgentChoice,
)
