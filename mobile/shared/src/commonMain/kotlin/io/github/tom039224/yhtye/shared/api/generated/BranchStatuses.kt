// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * The statuses of a repository's branches.
 */
@Serializable
data class BranchStatuses(
    /**
     * What merges are measured against (`origin`'s HEAD, else `main` /
     * `master` / the checked-out branch; `None`: no branch at all).
     */
    @SerialName("default_branch")
    val defaultBranch: String? = null,
    val statuses: List<BranchStatus>,
)
