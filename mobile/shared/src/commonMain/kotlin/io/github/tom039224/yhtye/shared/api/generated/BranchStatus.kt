// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The PR / merge status of one branch.
 */
@Serializable
data class BranchStatus(
    val branch: String,
    val state: BranchState,
    /**
     * The PR the state comes from (`None`: no PR, or unknown).
     */
    @SerialName("pr_number")
    val prNumber: Long? = null,
    @SerialName("pr_url")
    val prUrl: String? = null,
)
