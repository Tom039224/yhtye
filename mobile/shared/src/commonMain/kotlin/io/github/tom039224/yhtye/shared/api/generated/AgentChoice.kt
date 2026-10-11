// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * One harness × model × effort combination. `model: None` is the harness's
 * own default (for harnesses that expose no model option); `effort: None` means
 * "do not set the effort option" (Stage 7d, `core-design.md` §15.1). Old stored
 * values without `effort` read as `None`.
 */
@Serializable
data class AgentChoice(
    val harness: String,
    val model: String? = null,
    val effort: String? = null,
)
