// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * A registered harness, for the UI.
 */
@Serializable
data class HarnessInfo(
    val id: String,
    val label: String,
    /**
     * Every choice names a model (no "harness default" entry).
     */
    @SerialName("requires_model")
    val requiresModel: Boolean,
)
