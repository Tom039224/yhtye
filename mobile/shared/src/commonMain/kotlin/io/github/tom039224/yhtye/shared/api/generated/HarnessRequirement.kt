// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

/**
 * One command a harness needs, and where it was found.
 */
@Serializable
data class HarnessRequirement(
    val command: String,
    /**
     * The absolute path, `None` when not found.
     */
    val found: String? = null,
)
