// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Token usage of the coding agents on the machine the core runs on, read
 * from their own session logs (every session, not only those of Yhtye).
 */
@Serializable
data class UsageStats(
    /**
     * When it was computed (ms since the Unix epoch).
     */
    @SerialName("generated_at_ms")
    val generatedAtMs: Long,
    /**
     * The length of the period in days, ending today.
     */
    @SerialName("period_days")
    val periodDays: Long,
    /**
     * The UTC offset in minutes the days were cut with.
     */
    @SerialName("tz_offset_min")
    val tzOffsetMin: Int,
    val sources: List<StatsSource>,
    val totals: StatsTotals,
    /**
     * One entry per day of the period, oldest first, days without usage included.
     */
    val days: List<StatsDay>,
    /**
     * Most tokens first; at most 20, then an `other` entry.
     */
    @SerialName("by_model")
    val byModel: List<StatsBucket>,
    /**
     * Most tokens first.
     */
    @SerialName("by_harness")
    val byHarness: List<StatsBucket>,
    /**
     * Most tokens first; at most 20, then an `other` entry.
     */
    @SerialName("by_project")
    val byProject: List<StatsProject>,
)
