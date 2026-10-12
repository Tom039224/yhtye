// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

@Serializable(with = SessionStatus.Serializer::class)
enum class SessionStatus(override val wire: String) : WireEnum {
    Live("live"),
    /**
     * Stopped by Yhtye (task finished, review done) or its process exited.
     */
    Stopped("stopped"),
    /**
     * Stopped because Yhtye shut down.
     */
    Suspended("suspended"),
    /**
     * Found live or suspended on startup.
     */
    Interrupted("interrupted"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<SessionStatus>("SessionStatus", SessionStatus.entries, Unrecognized)
}
