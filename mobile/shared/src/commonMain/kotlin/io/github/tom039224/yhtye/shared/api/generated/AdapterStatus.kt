// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The adapter a harness is going to run, and why (the settings show it).
 */
@Serializable
data class AdapterStatus(
    /**
     * The npm package.
     */
    val `package`: String,
    /**
     * The version that was tested against Yhtye.
     */
    @SerialName("verified_version")
    val verifiedVersion: String,
    /**
     * What it resolved to.
     */
    val source: AdapterSource,
    /**
     * The version that runs; `None` for a global install (not asked).
     */
    val version: String? = null,
    /**
     * The package argument `npx -y` gets (`<package>@<version>`); `None` when
     * the global install runs.
     */
    val spec: String? = null,
    /**
     * The global install found on `PATH`, whichever channel is chosen.
     */
    @SerialName("global_path")
    val globalPath: String? = null,
    /**
     * Why the choice is not what runs (the latest version could not be
     * looked up, no global install was found, ...).
     */
    val note: String? = null,
)
