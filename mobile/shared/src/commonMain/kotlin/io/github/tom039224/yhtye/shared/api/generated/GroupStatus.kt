// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

@Serializable(with = GroupStatus.Serializer::class)
enum class GroupStatus(override val wire: String) : WireEnum {
    Active("active"),
    Finishing("finishing"),
    Done("done"),
    Cancelled("cancelled"),
    MergeBlocked("merge_blocked"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<GroupStatus>("GroupStatus", GroupStatus.entries, Unrecognized)
}
