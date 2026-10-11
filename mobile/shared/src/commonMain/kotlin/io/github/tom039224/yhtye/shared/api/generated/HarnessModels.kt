// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The models of one harness. Empty `models`: it has no model option (only its
 * own default can be used).
 */
@Serializable
data class HarnessModels(
    val harness: String,
    val models: List<ModelOption>,
    /**
     * The harness's current (default) model, if it reports one.
     */
    val current: String? = null,
    /**
     * When the harness was asked (ms since the Unix epoch).
     */
    @SerialName("fetched_at_ms")
    val fetchedAtMs: Long,
)
