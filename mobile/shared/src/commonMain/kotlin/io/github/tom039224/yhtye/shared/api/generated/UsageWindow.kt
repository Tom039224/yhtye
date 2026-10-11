// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class UsageWindow(
    val kind: UsageWindowKind,
    /**
     * Label as reported (`5-hour limit`, `Weekly · all models`, `Weekly · Opus`).
     */
    val label: String,
    /**
     * Utilization in percent (0–100).
     */
    val percent: Double,
    /**
     * When the window resets (ms since the Unix epoch), if it could be read.
     */
    @SerialName("resets_at_ms")
    val resetsAtMs: Long? = null,
)
