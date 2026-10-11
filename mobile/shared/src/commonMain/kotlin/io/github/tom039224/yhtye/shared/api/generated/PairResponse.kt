// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Answer of a successful `POST /pair`.
 */
@Serializable
data class PairResponse(
    @SerialName("device_id")
    val deviceId: String,
    /**
     * The device token: `Authorization: Bearer <token>` on `/ws`.
     */
    val token: String,
    /**
     * Identifies this Yhtye (the QR code's `sid`).
     */
    @SerialName("server_id")
    val serverId: String,
)
