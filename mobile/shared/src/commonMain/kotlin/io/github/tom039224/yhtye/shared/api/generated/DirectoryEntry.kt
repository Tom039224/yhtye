// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * A folder inside the listed one.
 */
@Serializable
data class DirectoryEntry(
    val name: String,
    /**
     * The folder's absolute path, to pass back as `path`.
     */
    val path: String,
    /**
     * It contains `.git` (a directory, or a file in a linked worktree).
     */
    @SerialName("is_git_repo")
    val isGitRepo: Boolean,
)
