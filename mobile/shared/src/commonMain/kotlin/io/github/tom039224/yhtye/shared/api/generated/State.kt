// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * Everything the state machine knows about one project.
 */
@Serializable
data class State(
    val project: String,
    val config: DomainConfig,
    /**
     * In creation order.
     */
    val chats: List<Chat>,
    val groups: List<Group>,
    val tasks: List<Task>,
    val helps: List<Help>,
    /**
     * Undelivered inbox entries, oldest first.
     */
    val inbox: List<InboxEntry>,
    val counters: Counters,
)
