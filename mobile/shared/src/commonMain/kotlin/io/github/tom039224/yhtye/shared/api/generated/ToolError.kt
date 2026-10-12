// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * A tool call that failed in a way the calling agent should read and fix.
 * Sent as `isError: true` with body `{"error": {"code", "message"}}`.
 */
@Serializable
data class ToolError(
    val code: ErrorCode,
    val message: String,
)
