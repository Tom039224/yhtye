// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class Group(
    val id: String,
    /**
     * The chat that created the group (`C-<n>`).
     */
    val chat: String,
    val title: String,
    val summary: String? = null,
    /**
     * Where the group is merged: the branch the chat's worktree had checked
     * out when the group was created (Stage 8e), or `finish_group`'s `into`.
     */
    @SerialName("base_branch")
    val baseBranch: String,
    @SerialName("group_branch")
    val groupBranch: String,
    val status: GroupStatus,
    /**
     * `finish_group`'s summary for the user.
     */
    @SerialName("finish_summary")
    val finishSummary: String? = null,
    /**
     * Result of the merge into the base branch, or why the group was cancelled.
     */
    val detail: String? = null,
    /**
     * Reminders sent because the orchestrator ended its turn while every task
     * had settled and the group was still open (Stage 7a; 0 in older logs).
     */
    @SerialName("finish_nudges")
    val finishNudges: Long = 0L,
)
