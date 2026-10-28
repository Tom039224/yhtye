// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * One calendar day of the client's time zone.
 */
@Serializable
data class StatsDay(
    /**
     * `YYYY-MM-DD`.
     */
    val date: String,
    val tokens: StatsTokens,
    /**
     * Sessions with a response that day (one that runs past midnight counts on both days).
     */
    val sessions: Long,
    /**
     * The harnesses that were used that day, by id; empty for a day without usage.
     */
    val harnesses: List<StatsDayHarness>,
)
