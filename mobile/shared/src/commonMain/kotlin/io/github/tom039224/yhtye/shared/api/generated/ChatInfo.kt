// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * A chat as the UI lists it (`Snapshot.chats`, `list_chats`). Its branch is
 * not stored: the UI reads what the worktree has checked out (Stage 8e).
 */
@Serializable
data class ChatInfo(
    val id: String,
    /**
     * The chat's worktree, as `git worktree list` shows it.
     */
    val worktree: String,
    val title: String? = null,
    @SerialName("created_ms")
    val createdMs: Long,
    /**
     * When the chat's orchestrator was last prompted (its creation until then).
     */
    @SerialName("last_used_ms")
    val lastUsedMs: Long,
)
