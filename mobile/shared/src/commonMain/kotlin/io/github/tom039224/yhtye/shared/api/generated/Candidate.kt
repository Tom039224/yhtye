// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * A candidate row: an [`AgentChoice`] plus a free-text `note` saying when to
 * use it (shown to the orchestrator, who picks by it). The same
 * harness × model may appear in several rows with different efforts.
 */
@Serializable
data class Candidate(
    val harness: String,
    val model: String? = null,
    val effort: String? = null,
    val note: String = "",
)
