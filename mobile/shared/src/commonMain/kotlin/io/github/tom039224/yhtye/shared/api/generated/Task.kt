// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).
@file:OptIn(ExperimentalSerializationApi::class)

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class Task(
    val id: String,
    val group: String,
    val title: String,
    val kind: TaskKind,
    @SerialName("depends_on")
    val dependsOn: List<String>,
    val instruction: String? = null,
    val steps: List<Step>,
    /**
     * Index of the step being run, or the next one to run.
     */
    val current: Long,
    val status: TaskStatus,
    @SerialName("review_rounds")
    val reviewRounds: Long,
    /**
     * Where the task's agents work (set once the git workspace is ready).
     */
    val workdir: String? = null,
    @SerialName("cancel_reason")
    val cancelReason: String? = null,
    /**
     * The orchestrator's choice of harness × model for the task's agent
     * (`create_task` `harness` / `model`, Stage 7b); `None` = the role default.
     */
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val agent: AgentChoice? = null,
    /**
     * The same for the task's review steps (`review_harness` / `review_model`).
     */
    @SerialName("review_agent")
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val reviewAgent: AgentChoice? = null,
)
