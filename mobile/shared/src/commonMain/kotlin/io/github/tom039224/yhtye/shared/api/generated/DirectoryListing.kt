// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The folders (not files) of one directory.
 */
@Serializable
data class DirectoryListing(
    /**
     * The listed directory, absolute with symbolic links resolved.
     */
    val path: String,
    /**
     * Its parent; `None` for the root.
     */
    val parent: String? = null,
    /**
     * The home directory of the user the core runs as.
     */
    val home: String,
    /**
     * The listed directory is itself a git repository.
     */
    @SerialName("is_git_repo")
    val isGitRepo: Boolean,
    /**
     * Sorted by name, ignoring case.
     */
    val entries: List<DirectoryEntry>,
)
