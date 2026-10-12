// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * Role of an agent session. Decides which MCP tools it sees.
 */
@Serializable(with = Role.Serializer::class)
enum class Role(override val wire: String) : WireEnum {
    Orchestrator("orchestrator"),
    Implementer("implementer"),
    Reviewer("reviewer"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<Role>("Role", Role.entries, Unrecognized)
}
