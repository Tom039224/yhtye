// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * One model a harness offers.
 */
@Serializable
data class ModelOption(
    /**
     * The value to select (`haiku`).
     */
    val value: String,
    /**
     * Display name.
     */
    val name: String,
    val description: String? = null,
    /**
     * The efforts this model supports; `None`: not read yet (ask with
     * `list_model_efforts`), empty: the model has no effort option.
     */
    val efforts: List<EffortOption>? = null,
)
