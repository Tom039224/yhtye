// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Which `claude` the Claude Code adapter drives, and why.
 */
@Serializable
data class ClaudeExecutable(
    /**
     * The user's `claude` that was found; `None` when none was.
     */
    val found: String? = null,
    @SerialName("path_source")
    val pathSource: HarnessPathSource,
    /**
     * The `claude` the adapter is told to run (`CLAUDE_CODE_EXECUTABLE`);
     * `None`: the Claude Code bundled with the adapter.
     */
    val using: String? = null,
    /**
     * Why the choice is not what runs.
     */
    val note: String? = null,
)
