// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

/**
 * One tool call as seen by the server (for logging / observation).
 */
@Serializable
data class ToolCallRecord(
    val binding: SessionBinding,
    val tool: String,
    val args: JsonElement,
    val result: ResultOfAnyValueOrToolError,
)
