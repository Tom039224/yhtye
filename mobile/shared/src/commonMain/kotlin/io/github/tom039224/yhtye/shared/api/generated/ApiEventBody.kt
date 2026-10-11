// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).
@file:OptIn(ExperimentalSerializationApi::class)

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.TaggedSerializer
import io.github.tom039224.yhtye.shared.api.TaggedVariant
import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

@Serializable(with = ApiEventBody.Serializer::class)
sealed interface ApiEventBody {
    /**
     * A state change (apply it with the same reducer as [`State::apply`]).
     */
    @Serializable
    data class Domain(
        val event: DomainEvent,
    ) : ApiEventBody

    @Serializable
    data class SessionStarted(
        val session: String,
        val role: Role,
        val task: String? = null,
        val pid: Long? = null,
        @SerialName("acp_session_id")
        val acpSessionId: String,
        /**
         * Restored with `session/load` (after a restart).
         */
        val resumed: Boolean,
        /**
         * The harness × model it runs (Stage 7b; absent in older logs).
         */
        val agent: AgentChoice? = null,
        /**
         * What it was meant to run when that harness is not registered (any
         * more) and `agent` runs instead (Stage 7c-2; shown to the user).
         */
        @EncodeDefault(EncodeDefault.Mode.NEVER)
        val replaced: AgentChoice? = null,
        /**
         * The directory it was started in (Stage 8; absent in older logs).
         */
        @EncodeDefault(EncodeDefault.Mode.NEVER)
        val cwd: String? = null,
        /**
         * The ACP session this start meant to restore but could not (it is a
         * new session instead); kept so the old one can be restored later.
         */
        @SerialName("lost_acp_session_id")
        @EncodeDefault(EncodeDefault.Mode.NEVER)
        val lostAcpSessionId: String? = null,
    ) : ApiEventBody

    /**
     * A start of the session failed. `retrying` tells a failure on the way from
     * the one that gives the start up.
     */
    @Serializable
    data class SessionFailed(
        val session: String,
        val error: String,
        /**
         * Another start of this session follows (a failed restore is tried
         * again, or replaced by a new session), so it is still starting. False:
         * the start is given up and nothing follows. Older logs lack it (the
         * two were not told apart); read it as false.
         */
        val retrying: Boolean,
    ) : ApiEventBody

    /**
     * The session was shut down (or its process exited) and its token revoked.
     */
    @Serializable
    data class SessionStopped(
        val session: String,
        /**
         * Stopped because Yhtye shut down; it is restored on the next start.
         */
        val suspended: Boolean,
    ) : ApiEventBody

    /**
     * On startup: the session was running (or suspended) when Yhtye stopped.
     * It is restored with `session/load` when its task resumes.
     */
    @Serializable
    data class SessionInterrupted(
        val session: String,
    ) : ApiEventBody

    /**
     * Agent output and lifecycle (stderr lines are not forwarded).
     */
    @Serializable
    data class Agent(
        val session: String,
        val event: AgentEvent,
    ) : ApiEventBody

    /**
     * A complete message / thought block of an agent (its chunks were live events).
     */
    @Serializable
    data class AgentText(
        val session: String,
        val kind: TextKind,
        val text: String,
    ) : ApiEventBody

    @Serializable
    data class ToolCalled(
        val record: ToolCallRecord,
    ) : ApiEventBody

    /**
     * A prompt was sent to a session (for the orchestrator: an inbox batch).
     */
    @Serializable
    data class Prompted(
        val session: String,
        val text: String,
    ) : ApiEventBody

    /**
     * App-wide (`project` is empty, `live`, `seq` 0): remote access started,
     * stopped, failed, or its pairing code was issued, used or dropped.
     * Local only: not sent to remote devices.
     */
    @Serializable
    data class RemoteStatusChanged(
        val status: RemoteStatus,
    ) : ApiEventBody

    /**
     * App-wide like `remote_status_changed`: a device was paired, revoked,
     * connected or disconnected. Local only.
     */
    @Serializable
    data class RemoteDevicesChanged(
        val devices: List<RemoteDevice>,
    ) : ApiEventBody

    /** A `type` this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(
        /** The unknown `type`. */
        val tag: String,
        override val raw: JsonObject,
    ) : ApiEventBody, UnrecognizedVariant

    object Serializer : TaggedSerializer<ApiEventBody>(
        "ApiEventBody",
        "type",
        listOf(
            TaggedVariant<ApiEventBody>("domain", Domain.serializer()) { it is Domain },
            TaggedVariant<ApiEventBody>("session_started", SessionStarted.serializer()) { it is SessionStarted },
            TaggedVariant<ApiEventBody>("session_failed", SessionFailed.serializer()) { it is SessionFailed },
            TaggedVariant<ApiEventBody>("session_stopped", SessionStopped.serializer()) { it is SessionStopped },
            TaggedVariant<ApiEventBody>("session_interrupted", SessionInterrupted.serializer()) { it is SessionInterrupted },
            TaggedVariant<ApiEventBody>("agent", Agent.serializer()) { it is Agent },
            TaggedVariant<ApiEventBody>("agent_text", AgentText.serializer()) { it is AgentText },
            TaggedVariant<ApiEventBody>("tool_called", ToolCalled.serializer()) { it is ToolCalled },
            TaggedVariant<ApiEventBody>("prompted", Prompted.serializer()) { it is Prompted },
            TaggedVariant<ApiEventBody>("remote_status_changed", RemoteStatusChanged.serializer()) { it is RemoteStatusChanged },
            TaggedVariant<ApiEventBody>("remote_devices_changed", RemoteDevicesChanged.serializer()) { it is RemoteDevicesChanged },
        ),
        ::Unrecognized,
    )
}
