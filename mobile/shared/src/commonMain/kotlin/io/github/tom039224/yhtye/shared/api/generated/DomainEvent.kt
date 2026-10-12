// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.TaggedSerializer
import io.github.tom039224.yhtye.shared.api.TaggedVariant
import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

@Serializable(with = DomainEvent.Serializer::class)
sealed interface DomainEvent {
    /**
     * A chat was created (Stage 8, `orchestration-model.md` §2.0).
     */
    @Serializable
    data class ChatCreated(
        val chat: Chat,
    ) : DomainEvent

    /**
     * The chat got its title: from its first user message, or the user's own
     * (`rename_chat`).
     */
    @Serializable
    data class ChatTitled(
        val chat: String,
        val title: String,
    ) : DomainEvent

    /**
     * The user deleted the chat. Its groups, their tasks and helps, and its
     * inbox go with it; the counters stay (ids are never reused).
     */
    @Serializable
    data class ChatDeleted(
        val chat: String,
    ) : DomainEvent

    @Serializable
    data class GroupCreated(
        val group: Group,
    ) : DomainEvent

    /**
     * `finish_group` with `into` (Stage 8e): the group now merges into `to`,
     * the branch its chat's worktree has checked out.
     */
    @Serializable
    data class GroupBaseChanged(
        val group: String,
        val from: String,
        val to: String,
    ) : DomainEvent

    @Serializable
    data class GroupFinishing(
        val group: String,
        val summary: String,
    ) : DomainEvent

    /**
     * The orchestrator ended its turn leaving a settled group open; it was
     * reminded to finish it (Stage 7a).
     */
    @Serializable
    data class GroupFinishReminded(
        val group: String,
    ) : DomainEvent

    /**
     * The group branch was merged into the base branch (`ok`) or could not be.
     */
    @Serializable
    data class GroupMergeFinished(
        val group: String,
        val ok: Boolean,
        val detail: String,
    ) : DomainEvent

    @Serializable
    data class GroupCancelled(
        val group: String,
        val reason: String,
    ) : DomainEvent

    @Serializable
    data class TaskCreated(
        val task: Task,
    ) : DomainEvent

    @Serializable
    data class InstructionSet(
        val task: String,
        val instruction: String,
    ) : DomainEvent

    @Serializable
    data class TaskStatusChanged(
        val task: String,
        val status: TaskStatus,
    ) : DomainEvent

    @Serializable
    data class TaskCancelled(
        val task: String,
        val reason: String,
    ) : DomainEvent

    @Serializable
    data class WorkspaceReady(
        val task: String,
        val path: String,
    ) : DomainEvent

    @Serializable
    data class StepStarted(
        val task: String,
        val step: Long,
        val note: String? = null,
    ) : DomainEvent

    @Serializable
    data class StepCompleted(
        val task: String,
        val step: Long,
        val result: String,
        val verdict: Verdict? = null,
    ) : DomainEvent

    /**
     * A step goes back to pending (e.g. `done` after a merge conflict).
     */
    @Serializable
    data class StepReset(
        val task: String,
        val step: Long,
    ) : DomainEvent

    /**
     * `modify_steps`: every step from `from` on is replaced.
     */
    @Serializable
    data class StepsReplaced(
        val task: String,
        val from: Long,
        val steps: List<StepSpec>,
    ) : DomainEvent

    /**
     * A `needs_changes` review inserted `steps` right after step `after`.
     */
    @Serializable
    data class ReviewStepsInserted(
        val task: String,
        val after: Long,
        val steps: List<StepSpec>,
    ) : DomainEvent

    @Serializable
    data class NudgeSent(
        val task: String,
        val step: Long,
    ) : DomainEvent

    /**
     * The orchestrator's `message_task` for the agent of step `step` was accepted
     * (log only: the state does not change).
     */
    @Serializable
    data class TaskMessageSent(
        val task: String,
        val step: Long,
        val text: String,
    ) : DomainEvent

    /**
     * What became of that message (log only). `via` is set when it was delivered.
     */
    @Serializable
    data class TaskMessageResolved(
        val task: String,
        val step: Long,
        val outcome: MessageOutcome,
        val via: MessageVia? = null,
        val detail: String,
    ) : DomainEvent

    @Serializable
    data class HelpRaised(
        val help: Help,
    ) : DomainEvent

    @Serializable
    data class HelpAnswered(
        val help: String,
        val action: HelpAction,
        val reply: String? = null,
    ) : DomainEvent

    @Serializable
    data class HelpClosed(
        val help: String,
    ) : DomainEvent

    @Serializable
    data class HelpAgentLost(
        val help: String,
    ) : DomainEvent

    @Serializable
    data class InboxQueued(
        val entry: InboxEntry,
    ) : DomainEvent

    /**
     * Every inbox entry of `chat` with `id <= up_to` was sent to its orchestrator.
     */
    @Serializable
    data class InboxDelivered(
        val chat: String,
        @SerialName("up_to")
        val upTo: Long,
    ) : DomainEvent

    /** A `type` this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(
        /** The unknown `type`. */
        val tag: String,
        override val raw: JsonObject,
    ) : DomainEvent, UnrecognizedVariant

    object Serializer : TaggedSerializer<DomainEvent>(
        "DomainEvent",
        "type",
        listOf(
            TaggedVariant<DomainEvent>("chat_created", ChatCreated.serializer()) { it is ChatCreated },
            TaggedVariant<DomainEvent>("chat_titled", ChatTitled.serializer()) { it is ChatTitled },
            TaggedVariant<DomainEvent>("chat_deleted", ChatDeleted.serializer()) { it is ChatDeleted },
            TaggedVariant<DomainEvent>("group_created", GroupCreated.serializer()) { it is GroupCreated },
            TaggedVariant<DomainEvent>("group_base_changed", GroupBaseChanged.serializer()) { it is GroupBaseChanged },
            TaggedVariant<DomainEvent>("group_finishing", GroupFinishing.serializer()) { it is GroupFinishing },
            TaggedVariant<DomainEvent>("group_finish_reminded", GroupFinishReminded.serializer()) { it is GroupFinishReminded },
            TaggedVariant<DomainEvent>("group_merge_finished", GroupMergeFinished.serializer()) { it is GroupMergeFinished },
            TaggedVariant<DomainEvent>("group_cancelled", GroupCancelled.serializer()) { it is GroupCancelled },
            TaggedVariant<DomainEvent>("task_created", TaskCreated.serializer()) { it is TaskCreated },
            TaggedVariant<DomainEvent>("instruction_set", InstructionSet.serializer()) { it is InstructionSet },
            TaggedVariant<DomainEvent>("task_status_changed", TaskStatusChanged.serializer()) { it is TaskStatusChanged },
            TaggedVariant<DomainEvent>("task_cancelled", TaskCancelled.serializer()) { it is TaskCancelled },
            TaggedVariant<DomainEvent>("workspace_ready", WorkspaceReady.serializer()) { it is WorkspaceReady },
            TaggedVariant<DomainEvent>("step_started", StepStarted.serializer()) { it is StepStarted },
            TaggedVariant<DomainEvent>("step_completed", StepCompleted.serializer()) { it is StepCompleted },
            TaggedVariant<DomainEvent>("step_reset", StepReset.serializer()) { it is StepReset },
            TaggedVariant<DomainEvent>("steps_replaced", StepsReplaced.serializer()) { it is StepsReplaced },
            TaggedVariant<DomainEvent>("review_steps_inserted", ReviewStepsInserted.serializer()) { it is ReviewStepsInserted },
            TaggedVariant<DomainEvent>("nudge_sent", NudgeSent.serializer()) { it is NudgeSent },
            TaggedVariant<DomainEvent>("task_message_sent", TaskMessageSent.serializer()) { it is TaskMessageSent },
            TaggedVariant<DomainEvent>("task_message_resolved", TaskMessageResolved.serializer()) { it is TaskMessageResolved },
            TaggedVariant<DomainEvent>("help_raised", HelpRaised.serializer()) { it is HelpRaised },
            TaggedVariant<DomainEvent>("help_answered", HelpAnswered.serializer()) { it is HelpAnswered },
            TaggedVariant<DomainEvent>("help_closed", HelpClosed.serializer()) { it is HelpClosed },
            TaggedVariant<DomainEvent>("help_agent_lost", HelpAgentLost.serializer()) { it is HelpAgentLost },
            TaggedVariant<DomainEvent>("inbox_queued", InboxQueued.serializer()) { it is InboxQueued },
            TaggedVariant<DomainEvent>("inbox_delivered", InboxDelivered.serializer()) { it is InboxDelivered },
        ),
        ::Unrecognized,
    )
}
