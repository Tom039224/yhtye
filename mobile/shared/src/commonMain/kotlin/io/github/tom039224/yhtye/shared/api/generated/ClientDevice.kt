// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * A paired device as every client may see it (no token, no hash, nothing to
 * manage it with).
 */
@Serializable
data class ClientDevice(
    val id: String,
    val name: String,
    val kind: ClientKind,
    /**
     * It is connected now.
     */
    val connected: Boolean,
    /**
     * When it last connected (`null`: never since pairing).
     */
    @SerialName("last_seen_ms")
    val lastSeenMs: Long? = null,
)
