// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * Answer of `list_clients` and content of `clients_changed`.
 */
@Serializable
data class Clients(
    val server: ClientServer,
    /**
     * The paired devices, oldest first.
     */
    val devices: List<ClientDevice>,
)
