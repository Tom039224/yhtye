// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).
@file:OptIn(ExperimentalSerializationApi::class)

package io.github.tom039224.yhtye.shared.api.generated

import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * One row of `agent_sessions` (also part of [`crate::api::Snapshot`]).
 */
@Serializable
data class SessionRecord(
    @SerialName("session_key")
    val sessionKey: String,
    val role: Role,
    val task: String? = null,
    @SerialName("acp_session_id")
    val acpSessionId: String,
    val status: SessionStatus,
    /**
     * A prompt was sent and its turn had not ended.
     */
    @SerialName("turn_running")
    val turnRunning: Boolean,
    /**
     * The harness × model × effort it ran (Stage 7b; `None` for older sessions).
     */
    val agent: AgentChoice? = null,
    /**
     * The directory the session was started in (Stage 8; `None` for older rows).
     */
    val cwd: String? = null,
    /**
     * The ACP session a start tried to restore and gave up on, replaced by
     * `acp_session_id`: kept to restore the lost session by hand later. It stays
     * until another restore is given up.
     */
    @SerialName("lost_acp_session_id")
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val lostAcpSessionId: String? = null,
)
