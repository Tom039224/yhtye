// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Whether a harness can be used, and why (the settings panel shows it).
 */
@Serializable
data class HarnessDetection(
    val id: String,
    val label: String,
    /**
     * Every command it needs was found (and a manual path, if any, is usable).
     */
    val installed: Boolean,
    /**
     * The main executable ([`HarnessSpec::main`]); `None` when not found.
     */
    @SerialName("resolved_path")
    val resolvedPath: String? = null,
    @SerialName("path_source")
    val pathSource: HarnessPathSource,
    /**
     * The path the user gave, even when it is unusable.
     */
    @SerialName("override_path")
    val overridePath: String? = null,
    /**
     * Why `override_path` cannot be used. The harness is then not installed:
     * the search does not replace a manual path that is wrong.
     */
    @SerialName("override_error")
    val overrideError: String? = null,
    val requirements: List<HarnessRequirement>,
)
