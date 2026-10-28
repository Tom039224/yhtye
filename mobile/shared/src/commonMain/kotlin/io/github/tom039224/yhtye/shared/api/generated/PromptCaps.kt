// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * What an agent accepts in a prompt (ACP `promptCapabilities`; the audio
 * capability is of no use to attachments).
 */
@Serializable
data class PromptCaps(
    /**
     * Image blocks.
     */
    val image: Boolean,
    /**
     * Embedded resource blocks (`embeddedContext`).
     */
    @SerialName("embedded_context")
    val embeddedContext: Boolean,
)
