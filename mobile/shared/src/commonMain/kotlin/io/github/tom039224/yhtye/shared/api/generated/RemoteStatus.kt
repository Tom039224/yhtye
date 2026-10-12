// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * What the settings panel shows of remote access.
 */
@Serializable
data class RemoteStatus(
    /**
     * The setting: remote access is wanted (it may still have failed to start).
     */
    val enabled: Boolean,
    /**
     * The configured port.
     */
    val port: Int,
    /**
     * The server is listening.
     */
    val running: Boolean,
    /**
     * Where it listens (`100.x.y.z:1423`, `[fd7a:…]:1423`); empty when not running.
     */
    val addresses: List<String>,
    /**
     * This PC's Tailscale IPv4 address (for the QR code), while running.
     */
    val ip: String? = null,
    /**
     * This PC's MagicDNS name (`pc.tailnet.ts.net`), while running and MagicDNS is on.
     */
    @SerialName("dns_name")
    val dnsName: String? = null,
    /**
     * Identifies this Yhtye to devices (random, made once).
     */
    @SerialName("server_id")
    val serverId: String,
    /**
     * Why it is not running although enabled (Tailscale missing or stopped,
     * the port taken, ...).
     */
    val error: String? = null,
    /**
     * Expiry of the pairing code that is valid now, if any (milliseconds since
     * the Unix epoch). The code itself is only in [`PairingCode`].
     */
    @SerialName("pairing_expires_at_ms")
    val pairingExpiresAtMs: Long? = null,
)
