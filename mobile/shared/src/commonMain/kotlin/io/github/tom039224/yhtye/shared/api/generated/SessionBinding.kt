// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).
@file:OptIn(ExperimentalSerializationApi::class)

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.Serializable

/**
 * Who is calling: the session's role and what it is bound to.
 */
@Serializable
data class SessionBinding(
    /**
     * Yhtye's key of the agent session (not the ACP session id).
     */
    val session: String,
    val role: Role,
    val project: String,
    /**
     * The chat an orchestrator session belongs to (Stage 8): its tools only
     * reach that chat's groups and tasks. `None` for sub-agents (they belong to
     * a task) and for the user's own actions (no restriction).
     */
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val chat: String? = null,
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val group: String? = null,
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val task: String? = null,
    /**
     * Index of the task step this session runs.
     */
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val step: Long? = null,
)
