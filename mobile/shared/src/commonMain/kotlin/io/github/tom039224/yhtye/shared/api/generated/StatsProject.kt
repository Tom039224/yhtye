// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * A project: a registered Yhtye project (its main clone or one of its
 * worktrees) or, for work outside any, the last directory name of the
 * session's working directory.
 */
@Serializable
data class StatsProject(
    /**
     * The project's name, or the directory name.
     */
    val name: String,
    /**
     * The id of the registered project; `None` for an unregistered directory.
     */
    val project: String? = null,
    val tokens: StatsTokens,
    val sessions: Long,
    /**
     * The sum of the entries cut from a long list.
     */
    val other: Boolean,
)
