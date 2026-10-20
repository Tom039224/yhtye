// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * A paired device as the settings panel lists it (never its token).
 */
@Serializable
data class RemoteDevice(
    val id: String,
    val name: String,
    val kind: ClientKind,
    @SerialName("created_ms")
    val createdMs: Long,
    /**
     * When it last connected (`null`: never since pairing).
     */
    @SerialName("last_seen_ms")
    val lastSeenMs: Long? = null,
    /**
     * It is connected now.
     */
    val connected: Boolean,
)
