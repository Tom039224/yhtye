// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * The efforts of one model of a harness.
 */
@Serializable
data class ModelEfforts(
    val harness: String,
    val model: String,
    val efforts: List<EffortOption>,
)
