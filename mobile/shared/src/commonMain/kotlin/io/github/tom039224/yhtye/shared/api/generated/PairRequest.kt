// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Body of `POST /pair`.
 */
@Serializable
data class PairRequest(
    /**
     * The pairing code shown on the PC.
     */
    val code: String,
    /**
     * The name the device is listed under (trimmed, at most
     * [`super::MAX_DEVICE_NAME_CHARS`] characters).
     */
    @SerialName("device_name")
    val deviceName: String,
)
