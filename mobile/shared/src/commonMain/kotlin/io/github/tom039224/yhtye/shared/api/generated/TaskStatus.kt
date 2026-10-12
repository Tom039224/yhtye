// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.WireEnum
import io.github.tom039224.yhtye.shared.api.WireEnumSerializer
import kotlinx.serialization.Serializable

@Serializable(with = TaskStatus.Serializer::class)
enum class TaskStatus(override val wire: String) : WireEnum {
    Pending("pending"),
    AwaitingInstruction("awaiting_instruction"),
    Running("running"),
    Checkpoint("checkpoint"),
    Handling("handling"),
    Merging("merging"),
    Done("done"),
    Cancelled("cancelled"),
    /**
     * Set on restart for tasks that were mid-turn or mid-merge (Stage 3b).
     */
    Interrupted("interrupted"),
    /** A value this client does not know (from a newer Yhtye). Cannot be sent. */
    Unrecognized("");

    object Serializer : WireEnumSerializer<TaskStatus>("TaskStatus", TaskStatus.entries, Unrecognized)
}
