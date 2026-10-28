// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * What one harness used on one day.
 */
@Serializable
data class StatsDayHarness(
    /**
     * `claude-code`, `codex` or `opencode`.
     */
    val harness: String,
    /**
     * Total tokens (all four kinds).
     */
    val tokens: Long,
    val sessions: Long,
)
