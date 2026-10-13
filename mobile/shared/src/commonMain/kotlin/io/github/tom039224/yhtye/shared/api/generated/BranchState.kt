// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * What the dot left of a branch in the BRANCHES tree says.
 */
@Serializable(with = BranchState.Serializer::class)
enum class BranchState(override val wire: String) : WireEnum {
    /**
     * No PR, and not merged into the default branch.
     */
    Unmerged("unmerged"),
    /**
     * An open PR exists (drafts included).
     */
    PrOpen("pr_open"),
    /**
     * Merged by a PR, with no changes since (the branch HEAD is the PR head
     * or its ancestor), or merged without any PR.
     */
    Merged("merged"),
    /**
     * Merged by a PR, but with commits past the PR head.
     */
    MergedWithChanges("merged_with_changes"),
    /**
     * Cannot tell (`gh` missing / unauthenticated / not GitHub / failed).
     */
    Unknown("unknown"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<BranchState>("BranchState", BranchState.entries, Unrecognized)
}
