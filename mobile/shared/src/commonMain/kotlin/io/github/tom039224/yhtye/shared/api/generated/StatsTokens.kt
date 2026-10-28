// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Token counts. `total` is `input + output + cache_read + cache_creation`.
 */
@Serializable
data class StatsTokens(
    /**
     * Input that was not served from the cache.
     */
    val input: Long,
    val output: Long,
    @SerialName("cache_read")
    val cacheRead: Long,
    @SerialName("cache_creation")
    val cacheCreation: Long,
    val total: Long,
)
