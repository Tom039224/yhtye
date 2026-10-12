// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The subscription usage reported by the harness.
 */
@Serializable
data class UsageReport(
    /**
     * Subscription plan as Claude Code names it (`max`, `pro`, ...), if reported.
     */
    val plan: String? = null,
    /**
     * Limit windows in the order reported (5-hour, weekly, per-model weekly).
     */
    val windows: List<UsageWindow>,
    /**
     * When the harness was asked (ms since the Unix epoch).
     */
    @SerialName("fetched_at_ms")
    val fetchedAtMs: Long,
)
