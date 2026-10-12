// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * Body of every refused HTTP request of the remote server (`/pair`, `/ws`).
 */
@Serializable
data class RemoteError(
    /**
     * Why, meant to be shown to the user.
     */
    val error: String,
)
