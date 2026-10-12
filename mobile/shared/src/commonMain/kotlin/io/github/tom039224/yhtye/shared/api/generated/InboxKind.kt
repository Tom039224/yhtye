// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

@Serializable(with = InboxKind.Serializer::class)
enum class InboxKind(override val wire: String) : WireEnum {
    UserMessage("user_message"),
    CheckpointReached("checkpoint_reached"),
    HelpRaised("help_raised"),
    InstructionNeeded("instruction_needed"),
    GroupSettled("group_settled"),
    MergeResult("merge_result"),
    /**
     * Yhtye restarted and the orchestrator missed something (Stage 3b).
     */
    Restarted("restarted"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<InboxKind>("InboxKind", InboxKind.entries, Unrecognized)
}
