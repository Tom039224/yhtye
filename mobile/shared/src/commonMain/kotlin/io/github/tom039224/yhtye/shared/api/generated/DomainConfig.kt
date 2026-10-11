// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class DomainConfig(
    /**
     * How many times a `needs_changes` review may insert implement + review.
     */
    @SerialName("max_review_rounds")
    val maxReviewRounds: Long,
)
