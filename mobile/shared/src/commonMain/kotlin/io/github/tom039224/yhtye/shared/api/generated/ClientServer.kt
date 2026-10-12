// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * The machine the core runs on.
 */
@Serializable
data class ClientServer(
    /**
     * Its host name (as `pong` tells).
     */
    val host: String,
)
