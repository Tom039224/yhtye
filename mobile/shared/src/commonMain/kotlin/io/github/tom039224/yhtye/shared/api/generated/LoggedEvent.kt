// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * A stored (durable) event, in the same shape as [`super::ApiEvent`].
 */
@Serializable
data class LoggedEvent(
    val seq: Long,
    @SerialName("ts_ms")
    val tsMs: Long,
    val project: String,
    /**
     * Always `false` (only durable events are stored).
     */
    val live: Boolean,
    val body: ApiEventBody,
)
