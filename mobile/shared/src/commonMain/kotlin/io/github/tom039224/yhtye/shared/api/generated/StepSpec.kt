// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).
@file:OptIn(ExperimentalSerializationApi::class)

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.Serializable

/**
 * One step of a task as given by the orchestrator.
 */
@Serializable
data class StepSpec(
    val kind: StepKind,
    /**
     * Instruction for this step; the task instruction is used when omitted.
     */
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val instruction: String? = null,
)
