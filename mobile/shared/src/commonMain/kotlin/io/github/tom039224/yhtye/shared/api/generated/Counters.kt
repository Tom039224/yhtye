// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * Id counters (the number of each kind created so far).
 */
@Serializable
data class Counters(
    val chats: Long = 0L,
    val groups: Long,
    val tasks: Long,
    val helps: Long,
    val inbox: Long,
)
