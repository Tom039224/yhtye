// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * A role as far as agent selection is concerned. `Investigator` is the agent
 * of an `investigate` task (a domain `Role::Implementer` session).
 */
@Serializable(with = AgentRole.Serializer::class)
enum class AgentRole(override val wire: String) : WireEnum {
    Orchestrator("orchestrator"),
    Implementer("implementer"),
    Investigator("investigator"),
    Reviewer("reviewer"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<AgentRole>("AgentRole", AgentRole.entries, Unrecognized)
}
