// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * A newly issued pairing code.
 */
@Serializable
data class PairingCode(
    val code: String,
    @SerialName("expires_at_ms")
    val expiresAtMs: Long,
    /**
     * The QR code's content:
     * `yhtye://pair?v=1&host=<MagicDNS name>&ip=<100.x>&port=<N>&code=<code>&sid=<server id>`
     * (`host` is left out without MagicDNS).
     */
    val uri: String,
)
