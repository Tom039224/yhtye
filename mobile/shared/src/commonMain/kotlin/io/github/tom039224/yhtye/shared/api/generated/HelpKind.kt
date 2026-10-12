// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

/**
 * Every kind of help: the agent kinds plus the ones Yhtye raises itself
 * (`mcp-tools.md` §4).
 */
@Serializable(with = HelpKind.Serializer::class)
enum class HelpKind(override val wire: String) : WireEnum {
    Blocked("blocked"),
    Question("question"),
    Policy("policy"),
    MergeConflict("merge_conflict"),
    ReviewRoundsExhausted("review_rounds_exhausted"),
    ProtocolViolation("protocol_violation"),
    AgentStopped("agent_stopped"),
    AgentCrashed("agent_crashed"),
    DirtyReadonlyTree("dirty_readonly_tree"),
    /**
     * A git operation failed for a reason other than a conflict (Stage 3a addition).
     */
    GitFailed("git_failed"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<HelpKind>("HelpKind", HelpKind.entries, Unrecognized)
}
