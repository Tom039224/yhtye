// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.Serializable

@Serializable
data class Step(
    val kind: StepKind,
    val instruction: String? = null,
    val status: StepStatus,
    val result: String? = null,
    val verdict: Verdict? = null,
    /**
     * Note from the orchestrator given when the step (re)started.
     */
    val note: String? = null,
    /**
     * Reminders sent because a turn ended without `report_step_done` / `help`.
     */
    val nudges: Long,
)
