// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * Client → bridge: run `cmd`; the reply carries the same `id`.
 */
@Serializable
data class WsRequest(
    val id: Long,
    val cmd: ApiCommand,
)
