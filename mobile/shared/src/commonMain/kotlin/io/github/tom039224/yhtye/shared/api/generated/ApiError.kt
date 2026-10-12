// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * A failed command. `message` is meant to be shown to the user.
 */
@Serializable
data class ApiError(
    val code: ApiErrorCode,
    val message: String,
)
