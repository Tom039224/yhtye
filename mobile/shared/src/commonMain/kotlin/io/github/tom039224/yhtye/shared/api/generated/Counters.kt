// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).
@file:OptIn(ExperimentalSerializationApi::class)

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Id counters (the number of each kind created so far), and the names that
 * are gone from the state but may not be used again.
 */
@Serializable
data class Counters(
    val chats: Long = 0L,
    /**
     * Groups created (their ids are names now; older ones are `G-<n>`).
     */
    val groups: Long,
    /**
     * Tasks created (their ids are names now; older ones are `T-<n>`).
     */
    val tasks: Long,
    val helps: Long,
    val inbox: Long,
    /**
     * Names of the groups and tasks of deleted chats: names are unique in the
     * project for good, so these are never given again (empty in older stores;
     * left out when empty, so clients that do not know it see no change).
     */
    @SerialName("retired_names")
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val retiredNames: List<String> = emptyList(),
)
