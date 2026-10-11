// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

@Serializable(with = ApiErrorCode.Serializer::class)
enum class ApiErrorCode(override val wire: String) : WireEnum {
    InvalidArgument("invalid_argument"),
    NotFound("not_found"),
    InvalidState("invalid_state"),
    Conflict("conflict"),
    Forbidden("forbidden"),
    Internal("internal"),
    /**
     * The project's orchestration (or the core) has stopped.
     */
    Unavailable("unavailable"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<ApiErrorCode>("ApiErrorCode", ApiErrorCode.entries, Unrecognized)
}
