// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The repository as the git panel shows it.
 */
@Serializable
data class GitOverview(
    /**
     * Branch checked out in the project directory (`None`: detached HEAD or no commits).
     */
    val head: String? = null,
    /**
     * Commit of `HEAD` (`None`: no commits yet).
     */
    @SerialName("head_sha")
    val headSha: String? = null,
    /**
     * Local branches, by name.
     */
    val branches: List<GitBranch>,
    /**
     * Every worktree of the repository, the main one first (Stage 8e: the
     * BRANCHES tree and the branch shown for a chat come from here).
     */
    val worktrees: List<GitWorktree>,
    /**
     * Newest first, parents after their children (`--topo-order`).
     */
    val commits: List<GitCommit>,
    /**
     * More commits exist beyond `commits`.
     */
    val truncated: Boolean,
)
