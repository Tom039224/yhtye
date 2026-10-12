// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

@Serializable(with = TaskKind.Serializer::class)
enum class TaskKind(override val wire: String) : WireEnum {
    /**
     * Writes code in a dedicated worktree; merged into the group branch when done.
     */
    Code("code"),
    /**
     * Read-only investigation; only the result text is kept.
     */
    Investigate("investigate"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<TaskKind>("TaskKind", TaskKind.entries, Unrecognized)
}
