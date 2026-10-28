// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * A model or a harness.
 */
@Serializable
data class StatsBucket(
    /**
     * The model name as the agent logged it, or the harness id. `other` is
     * the sum of the entries cut from a long list (and `other` is `true`).
     */
    val name: String,
    val tokens: StatsTokens,
    val sessions: Long,
    val other: Boolean,
)
