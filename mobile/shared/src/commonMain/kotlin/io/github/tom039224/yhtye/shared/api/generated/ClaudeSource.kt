// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * Which Claude Code the adapter drives.
 */
@Serializable(with = ClaudeSource.Serializer::class)
enum class ClaudeSource(override val wire: String) : WireEnum {
    /**
     * The user's own `claude` (`CLAUDE_CODE_EXECUTABLE`), when one is found.
     */
    User("user"),
    /**
     * The Claude Code bundled with the adapter.
     */
    Bundled("bundled"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<ClaudeSource>("ClaudeSource", ClaudeSource.entries, Unrecognized)
}
