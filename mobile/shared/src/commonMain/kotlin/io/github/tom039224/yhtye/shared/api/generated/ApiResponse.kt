// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.TaggedSerializer
import io.github.tom039224.yhtye.shared.api.TaggedVariant
import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

@Serializable(with = ApiResponse.Serializer::class)
sealed interface ApiResponse {
    /**
     * The core is alive; `host` is the name of the machine it runs on.
     */
    @Serializable
    data class Pong(
        val host: String,
    ) : ApiResponse

    @Serializable
    data class Projects(
        val projects: List<ProjectInfo>,
    ) : ApiResponse

    @Serializable
    data class Project(
        val project: ProjectInfo,
    ) : ApiResponse

    @Serializable
    data class Snapshot(
        val snapshot: io.github.tom039224.yhtye.shared.api.generated.Snapshot,
    ) : ApiResponse

    @Serializable
    data class Events(
        val events: List<LoggedEvent>,
        /**
         * More events follow (the page was full).
         */
        val more: Boolean,
    ) : ApiResponse

    /**
     * The command was accepted (sending, cancelling).
     */
    @Serializable
    data object Accepted : ApiResponse

    @Serializable
    data class Chats(
        val chats: List<ChatInfo>,
    ) : ApiResponse

    @Serializable
    data class Chat(
        val chat: ChatInfo,
    ) : ApiResponse

    /**
     * Answer of `open_chat`.
     */
    @Serializable
    data class ChatOpened(
        /**
         * The chat's orchestrator is being started (now, or by an earlier call).
         */
        val starting: Boolean,
    ) : ApiResponse

    @Serializable
    data class GitOverview(
        val git: io.github.tom039224.yhtye.shared.api.generated.GitOverview,
    ) : ApiResponse

    @Serializable
    data class Usage(
        val usage: UsageReport,
    ) : ApiResponse

    @Serializable
    data class AgentSettings(
        val settings: AgentSettingsView,
    ) : ApiResponse

    @Serializable
    data class HarnessModels(
        val models: io.github.tom039224.yhtye.shared.api.generated.HarnessModels,
    ) : ApiResponse

    @Serializable
    data class ModelEfforts(
        val efforts: io.github.tom039224.yhtye.shared.api.generated.ModelEfforts,
    ) : ApiResponse

    /**
     * The registered secret environment variable names, sorted.
     */
    @Serializable
    data class SecretEnv(
        val names: List<String>,
    ) : ApiResponse

    /**
     * Every known harness with whether it was found and where. Empty when the
     * core does not detect harnesses (tests).
     */
    @Serializable
    data class Harnesses(
        val harnesses: List<HarnessDetection>,
    ) : ApiResponse

    /**
     * Answer of `get_remote_status` and `set_remote_settings`.
     */
    @Serializable
    data class RemoteStatus(
        val status: io.github.tom039224.yhtye.shared.api.generated.RemoteStatus,
    ) : ApiResponse

    /**
     * Answer of `create_pairing_code`.
     */
    @Serializable
    data class PairingCode(
        val pairing: io.github.tom039224.yhtye.shared.api.generated.PairingCode,
    ) : ApiResponse

    /**
     * Answer of `list_remote_devices` and `revoke_remote_device`.
     */
    @Serializable
    data class RemoteDevices(
        val devices: List<RemoteDevice>,
    ) : ApiResponse

    /** A `type` this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(
        /** The unknown `type`. */
        val tag: String,
        override val raw: JsonObject,
    ) : ApiResponse, UnrecognizedVariant

    object Serializer : TaggedSerializer<ApiResponse>(
        "ApiResponse",
        "type",
        listOf(
            TaggedVariant<ApiResponse>("pong", Pong.serializer()) { it is Pong },
            TaggedVariant<ApiResponse>("projects", Projects.serializer()) { it is Projects },
            TaggedVariant<ApiResponse>("project", Project.serializer()) { it is Project },
            TaggedVariant<ApiResponse>("snapshot", Snapshot.serializer()) { it is Snapshot },
            TaggedVariant<ApiResponse>("events", Events.serializer()) { it is Events },
            TaggedVariant<ApiResponse>("accepted", Accepted.serializer()) { it is Accepted },
            TaggedVariant<ApiResponse>("chats", Chats.serializer()) { it is Chats },
            TaggedVariant<ApiResponse>("chat", Chat.serializer()) { it is Chat },
            TaggedVariant<ApiResponse>("chat_opened", ChatOpened.serializer()) { it is ChatOpened },
            TaggedVariant<ApiResponse>("git_overview", GitOverview.serializer()) { it is GitOverview },
            TaggedVariant<ApiResponse>("usage", Usage.serializer()) { it is Usage },
            TaggedVariant<ApiResponse>("agent_settings", AgentSettings.serializer()) { it is AgentSettings },
            TaggedVariant<ApiResponse>("harness_models", HarnessModels.serializer()) { it is HarnessModels },
            TaggedVariant<ApiResponse>("model_efforts", ModelEfforts.serializer()) { it is ModelEfforts },
            TaggedVariant<ApiResponse>("secret_env", SecretEnv.serializer()) { it is SecretEnv },
            TaggedVariant<ApiResponse>("harnesses", Harnesses.serializer()) { it is Harnesses },
            TaggedVariant<ApiResponse>("remote_status", RemoteStatus.serializer()) { it is RemoteStatus },
            TaggedVariant<ApiResponse>("pairing_code", PairingCode.serializer()) { it is PairingCode },
            TaggedVariant<ApiResponse>("remote_devices", RemoteDevices.serializer()) { it is RemoteDevices },
        ),
        ::Unrecognized,
    )
}
