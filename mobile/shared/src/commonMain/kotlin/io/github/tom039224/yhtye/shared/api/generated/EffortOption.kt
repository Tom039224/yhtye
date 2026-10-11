// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * One effort (thought level) a model supports.
 */
@Serializable
data class EffortOption(
    /**
     * The value to select (`high`).
     */
    val value: String,
    val name: String,
    val description: String? = null,
)
