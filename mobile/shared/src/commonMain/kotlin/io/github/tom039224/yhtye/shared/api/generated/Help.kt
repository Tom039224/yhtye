// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class Help(
    val id: String,
    val task: String,
    val step: Long,
    val kind: HelpKind,
    val message: String,
    val source: HelpSource,
    val state: HelpState,
    /**
     * The asking agent's process died while waiting for the answer.
     */
    @SerialName("agent_lost")
    val agentLost: Boolean,
    val reply: String? = null,
)
