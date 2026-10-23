// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * The whole period.
 */
@Serializable
data class StatsTotals(
    val tokens: StatsTokens,
    /**
     * Sessions with at least one response in the period.
     */
    val sessions: Long,
)
