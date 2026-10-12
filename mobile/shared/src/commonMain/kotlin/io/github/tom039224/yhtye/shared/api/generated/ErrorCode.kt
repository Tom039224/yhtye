// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * Error codes of tool errors (`mcp-tools.md` §1).
 */
@Serializable(with = ErrorCode.Serializer::class)
enum class ErrorCode(override val wire: String) : WireEnum {
    InvalidArgument("invalid_argument"),
    NotFound("not_found"),
    InvalidState("invalid_state"),
    Forbidden("forbidden"),
    Conflict("conflict"),
    Internal("internal"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<ErrorCode>("ErrorCode", ErrorCode.entries, Unrecognized)
}
