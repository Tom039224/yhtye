// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * The state of a project as of event `seq`: the domain state, the agent
 * sessions (`agent_sessions`, derived from the same events) and the chats.
 */
@Serializable
data class Snapshot(
    val seq: Long,
    val state: State,
    val sessions: List<SessionRecord>,
    /**
     * The project's chats with their times, most recently used first (Stage 8).
     */
    val chats: List<ChatInfo>,
)
