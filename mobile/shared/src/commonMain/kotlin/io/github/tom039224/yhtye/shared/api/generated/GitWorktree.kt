// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * One worktree as `git worktree list --porcelain` shows it.
 */
@Serializable
data class GitWorktree(
    /**
     * As git shows it (what a chat's `worktree` is compared with).
     */
    val path: String,
    /**
     * Checked-out branch (`None`: detached HEAD).
     */
    val branch: String? = null,
    /**
     * Checked-out commit (`None`: no commits yet).
     */
    @SerialName("head_sha")
    val headSha: String? = null,
    /**
     * The repository's main worktree (the original clone).
     */
    @SerialName("is_main")
    val isMain: Boolean,
    /**
     * Its directory is gone.
     */
    val missing: Boolean,
)
