// Generated from the Rust API types of yhtye-core (crates/yhtye-core/src/api/typegen/kotlin).
// Do not edit; run `pnpm gen:kotlin` (or `pnpm gen:types`).

package io.github.tom039224.yhtye.shared.api.generated

import io.github.tom039224.yhtye.shared.api.TaggedSerializer
import io.github.tom039224.yhtye.shared.api.TaggedVariant
import io.github.tom039224.yhtye.shared.api.UnrecognizedVariant
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

@Serializable(with = ApiCommand.Serializer::class)
sealed interface ApiCommand {
    /**
     * A liveness check of the core: answers with the name of the machine it
     * runs on (the host footer of the sidebar). The UI sends it periodically.
     */
    @Serializable
    data object Ping : ApiCommand

    /**
     * Every project known to the database, and whether it is open.
     */
    @Serializable
    data object ListProjects : ApiCommand

    /**
     * Opens (registering it on first use) the git repository at `path`: starts
     * its orchestrator and resumes interrupted tasks. Opening an open project
     * returns it unchanged.
     */
    @Serializable
    data class OpenProject(
        val path: String,
    ) : ApiCommand

    @Serializable
    data class GetSnapshot(
        val project: String,
    ) : ApiCommand

    /**
     * Durable events with `seq > after_seq`, oldest first (history and
     * catch-up after a gap). Works for projects that are not open.
     */
    @Serializable
    data class ListEvents(
        val project: String,
        @SerialName("after_seq")
        val afterSeq: Long,
        val limit: Long? = null,
    ) : ApiCommand

    /**
     * Queues a `user_message` for the orchestrator of `chat` (sent when it is
     * idle). Starts that orchestrator if it is not running (Stage 8: lazily,
     * restoring the chat's stored session if there is one). `not_found` for an
     * unknown chat, `invalid_state` when the chat's worktree no longer exists.
     */
    @Serializable
    data class SendUserMessage(
        val project: String,
        val chat: String,
        val text: String,
    ) : ApiCommand

    /**
     * The user opened `chat` in the UI: its orchestrator is started now,
     * restoring its stored session, if it is not running and has anything to
     * resume (it ran when Yhtye stopped, the chat has an open group, or it has
     * inbox entries). An app restart starts no orchestrator; this is where a
     * chat comes back (also when something is sent to it). Answers
     * `chat_opened`; the start itself goes on in the background and shows as
     * `session_started` (or `session_failed`). `not_found` for an unknown chat.
     */
    @Serializable
    data class OpenChat(
        val project: String,
        val chat: String,
    ) : ApiCommand

    /**
     * Cancels the running turn of the chat's orchestrator, if any.
     */
    @Serializable
    data class CancelOrchestratorTurn(
        val project: String,
        val chat: String,
    ) : ApiCommand

    /**
     * Sends [`crate::domain::COMPACT_PROMPT`] (`/compact`) as is, not through
     * the inbox, to the chat's orchestrator session: the harness compacts its
     * context (Claude Code does; others may answer it as a plain message).
     * Inbox entries that arrive meanwhile are delivered after that turn.
     * `not_found` for an unknown chat, `invalid_state` unless the orchestrator
     * is running and idle (not started, starting, or in a turn).
     */
    @Serializable
    data class CompactChat(
        val project: String,
        val chat: String,
    ) : ApiCommand

    /**
     * Stops the chat's orchestrator session (its process ends and its token is
     * revoked), so the chat rests until it is used again: the next message to
     * it starts the orchestrator again, restoring the stored session. Sessions
     * start lazily, so one that was never started is already asleep.
     * `not_found` for an unknown chat, `invalid_state` while its orchestrator
     * is working (starting or in a turn); stop or wait for the turn first.
     * Stopping an idle session reports `session_stopped`, like any other stop.
     */
    @Serializable
    data class SleepChat(
        val project: String,
        val chat: String,
    ) : ApiCommand

    /**
     * The project's chats, most recently used first. Works for projects that
     * are not open.
     */
    @Serializable
    data class ListChats(
        val project: String,
    ) : ApiCommand

    /**
     * A new chat (its orchestrator is not started), bound to a worktree
     * (Stage 8e): give exactly one of `branch` (its worktree: where it is
     * checked out, else a new Yhtye one) or `worktree` (an existing worktree,
     * as `get_git_overview` lists it). `not_found` for a missing branch or an
     * unknown worktree, `invalid_argument` for `yhtye/&#42;`, for both or neither.
     */
    @Serializable
    data class CreateChat(
        val project: String,
        val branch: String? = null,
        val worktree: String? = null,
    ) : ApiCommand

    /**
     * Gives a chat the user's own title (whitespace is flattened; at most
     * [`crate::domain::MAX_RENAMED_TITLE_CHARS`] characters), which the first
     * message no longer replaces. `invalid_argument` for an empty or too long
     * title, `not_found` for an unknown chat. The project must be open.
     */
    @Serializable
    data class RenameChat(
        val project: String,
        val chat: String,
        val title: String,
    ) : ApiCommand

    /**
     * Deletes a chat: its stored conversation state, its finished groups with
     * their tasks, its inbox and its orchestrator's stored session. Its
     * worktree and branch are not touched, and the event log keeps the history
     * (the log is append-only). `not_found` for an unknown chat,
     * `invalid_state` while its orchestrator is working or it has a group that
     * is not finished (`active`, `finishing`, `merge_blocked`); an idle
     * orchestrator process is stopped. The project must be open.
     */
    @Serializable
    data class DeleteChat(
        val project: String,
        val chat: String,
    ) : ApiCommand

    /**
     * Creates a branch in a Yhtye worktree (the main clone keeps its checkout)
     * and its first chat. `from` defaults to the main worktree's HEAD.
     * `invalid_argument` for a bad or reserved (`yhtye/&#42;`) name, `conflict` if
     * the branch exists, `not_found` if `from` does not.
     */
    @Serializable
    data class CreateBranch(
        val project: String,
        val name: String,
        val from: String? = null,
    ) : ApiCommand

    /**
     * Cancels a task as the user (like the orchestrator's `cancel_task`); the
     * orchestrator is told with a `user_message`.
     */
    @Serializable
    data class CancelTask(
        val project: String,
        val task: String,
        val reason: String? = null,
    ) : ApiCommand

    /**
     * Cancels a group as the user (like `cancel_group`); the orchestrator is told.
     */
    @Serializable
    data class CancelGroup(
        val project: String,
        val group: String,
        val reason: String? = null,
    ) : ApiCommand

    /**
     * Retries the base merge of a `merge_blocked` group. Accepted means the
     * merge ran; whether it succeeded is in the group's status (and the
     * orchestrator gets a `merge_result`).
     */
    @Serializable
    data class RetryGroupMerge(
        val project: String,
        val group: String,
    ) : ApiCommand

    /**
     * The project repository's checked-out branch, local branches and recent
     * commit graph (the git panel, Stage 6a). Works for projects that are not open.
     */
    @Serializable
    data class GetGitOverview(
        val project: String,
        /**
         * Most commits to return (default [`DEFAULT_GRAPH_COMMITS`], at most 500).
         */
        val limit: Long? = null,
    ) : ApiCommand

    /**
     * Subscription usage / quota of the harness (5-hour and weekly windows,
     * plan name), Stage 6b. Cached for a minute; `refresh` asks again unless
     * the cached report is only seconds old. `unavailable` when the harness
     * does not report it.
     */
    @Serializable
    data class GetUsage(
        val refresh: Boolean? = null,
    ) : ApiCommand

    /**
     * The harness × model settings of every role (Stage 7b): registered
     * harnesses, the global layer and, with `project`, its layer and the
     * settings in effect there (the project must be registered, not open).
     */
    @Serializable
    data class GetAgentSettings(
        val project: String? = null,
    ) : ApiCommand

    /**
     * Replaces one role of the global layer (no `project`) or a project's
     * layer; `settings: null` removes it (inherit). Applies to sessions
     * started afterwards. Returns the settings like `get_agent_settings`.
     * With `expected_revision` (the `revision` of the settings the screen
     * shows), `conflict` if they were changed since (by another client), so an
     * old screen does not overwrite them.
     */
    @Serializable
    data class SetAgentSettings(
        val project: String? = null,
        val role: AgentRole,
        val settings: RoleSettings? = null,
        @SerialName("expected_revision")
        val expectedRevision: Long? = null,
    ) : ApiCommand

    /**
     * The models a harness offers, read from it over ACP (a short session
     * without a prompt). Cached for 10 minutes; `refresh` asks again unless
     * the cached list is only seconds old.
     */
    @Serializable
    data class ListHarnessModels(
        val harness: String,
        val refresh: Boolean? = null,
    ) : ApiCommand

    /**
     * The efforts (thought levels) one model of a harness supports, for
     * models whose efforts `list_harness_models` did not include (harnesses
     * with many models). Cached for 10 minutes; costs one short listing
     * session the first time.
     */
    @Serializable
    data class ListModelEfforts(
        val harness: String,
        val model: String,
    ) : ApiCommand

    /**
     * Which harnesses are installed: looks for their executables again (the
     * `PATH`, a few well-known directories, and the paths set with
     * `set_harness_path`) and registers those that were found, so the settings
     * panel offers them. Sessions already running are not affected. Called when
     * the settings are opened.
     */
    @Serializable
    data object GetHarnesses : ApiCommand

    /**
     * Like `get_harnesses`, and also forgets the models read from every
     * harness (the "detect again" button).
     */
    @Serializable
    data object DetectHarnesses : ApiCommand

    /**
     * Sets (or with `path: null` removes: detect automatically) the path of a
     * harness's main executable (`npx` for Claude Code), used before the `PATH`
     * search. `invalid_argument` unless it is an absolute path of an executable
     * file, `not_found` for an unknown harness. Detects again and returns the
     * harnesses like `get_harnesses`.
     */
    @Serializable
    data class SetHarnessPath(
        val harness: String,
        val path: String? = null,
    ) : ApiCommand

    /**
     * The names of the secret environment variables (Stage 7e). Values are
     * never returned.
     */
    @Serializable
    data object ListSecretEnv : ApiCommand

    /**
     * Stores `value` in the OS credential store under `name` (replacing an
     * earlier one) and registers the name; every agent started afterwards gets
     * the variable. `unavailable` when the credential store cannot be used.
     */
    @Serializable
    data class SetSecretEnv(
        val name: String,
        val value: String,
    ) : ApiCommand

    /**
     * Removes the value and the name.
     */
    @Serializable
    data class DeleteSecretEnv(
        val name: String,
    ) : ApiCommand

    /**
     * Remote access (`core-design.md` §18): its settings, whether it is
     * listening and where, and why not. Local only (like every `*remote*` /
     * pairing command; see [`ApiCommand::is_local_only`]).
     */
    @Serializable
    data object GetRemoteStatus : ApiCommand

    /**
     * Turns remote access on or off and sets its port (`port` absent: keep
     * it; default 1423). Stored, and applied at once: starts, restarts or stops
     * the server. Turning it on when it failed to start tries again. A failure
     * to start (Tailscale missing or stopped, the port taken) is not an error
     * of the command: it is the status's `error`. `invalid_argument` for port 0.
     */
    @Serializable
    data class SetRemoteSettings(
        val enabled: Boolean,
        val port: Int? = null,
    ) : ApiCommand

    /**
     * Issues a pairing code (valid for 5 minutes, once; the previous code stops
     * working) and the QR code's URI. `invalid_state` unless the server is running.
     */
    @Serializable
    data object CreatePairingCode : ApiCommand

    /**
     * The paired devices, oldest first.
     */
    @Serializable
    data object ListRemoteDevices : ApiCommand

    /**
     * Forgets a paired device and closes its connections at once; its token
     * stops working. `not_found` for an unknown device.
     */
    @Serializable
    data class RevokeRemoteDevice(
        val device: String,
    ) : ApiCommand

    /**
     * This PC (its host name) and the paired devices with whether each is
     * connected (`core-design.md` §18.8): a read-only view for every client,
     * remote devices too (not local only). `clients_changed` says when it changed.
     */
    @Serializable
    data object ListClients : ApiCommand

    /** A `type` this client does not know (from a newer Yhtye). Cannot be sent. */
    data class Unrecognized(
        /** The unknown `type`. */
        val tag: String,
        override val raw: JsonObject,
    ) : ApiCommand, UnrecognizedVariant

    object Serializer : TaggedSerializer<ApiCommand>(
        "ApiCommand",
        "type",
        listOf(
            TaggedVariant<ApiCommand>("ping", Ping.serializer()) { it is Ping },
            TaggedVariant<ApiCommand>("list_projects", ListProjects.serializer()) { it is ListProjects },
            TaggedVariant<ApiCommand>("open_project", OpenProject.serializer()) { it is OpenProject },
            TaggedVariant<ApiCommand>("get_snapshot", GetSnapshot.serializer()) { it is GetSnapshot },
            TaggedVariant<ApiCommand>("list_events", ListEvents.serializer()) { it is ListEvents },
            TaggedVariant<ApiCommand>("send_user_message", SendUserMessage.serializer()) { it is SendUserMessage },
            TaggedVariant<ApiCommand>("open_chat", OpenChat.serializer()) { it is OpenChat },
            TaggedVariant<ApiCommand>("cancel_orchestrator_turn", CancelOrchestratorTurn.serializer()) { it is CancelOrchestratorTurn },
            TaggedVariant<ApiCommand>("compact_chat", CompactChat.serializer()) { it is CompactChat },
            TaggedVariant<ApiCommand>("sleep_chat", SleepChat.serializer()) { it is SleepChat },
            TaggedVariant<ApiCommand>("list_chats", ListChats.serializer()) { it is ListChats },
            TaggedVariant<ApiCommand>("create_chat", CreateChat.serializer()) { it is CreateChat },
            TaggedVariant<ApiCommand>("rename_chat", RenameChat.serializer()) { it is RenameChat },
            TaggedVariant<ApiCommand>("delete_chat", DeleteChat.serializer()) { it is DeleteChat },
            TaggedVariant<ApiCommand>("create_branch", CreateBranch.serializer()) { it is CreateBranch },
            TaggedVariant<ApiCommand>("cancel_task", CancelTask.serializer()) { it is CancelTask },
            TaggedVariant<ApiCommand>("cancel_group", CancelGroup.serializer()) { it is CancelGroup },
            TaggedVariant<ApiCommand>("retry_group_merge", RetryGroupMerge.serializer()) { it is RetryGroupMerge },
            TaggedVariant<ApiCommand>("get_git_overview", GetGitOverview.serializer()) { it is GetGitOverview },
            TaggedVariant<ApiCommand>("get_usage", GetUsage.serializer()) { it is GetUsage },
            TaggedVariant<ApiCommand>("get_agent_settings", GetAgentSettings.serializer()) { it is GetAgentSettings },
            TaggedVariant<ApiCommand>("set_agent_settings", SetAgentSettings.serializer()) { it is SetAgentSettings },
            TaggedVariant<ApiCommand>("list_harness_models", ListHarnessModels.serializer()) { it is ListHarnessModels },
            TaggedVariant<ApiCommand>("list_model_efforts", ListModelEfforts.serializer()) { it is ListModelEfforts },
            TaggedVariant<ApiCommand>("get_harnesses", GetHarnesses.serializer()) { it is GetHarnesses },
            TaggedVariant<ApiCommand>("detect_harnesses", DetectHarnesses.serializer()) { it is DetectHarnesses },
            TaggedVariant<ApiCommand>("set_harness_path", SetHarnessPath.serializer()) { it is SetHarnessPath },
            TaggedVariant<ApiCommand>("list_secret_env", ListSecretEnv.serializer()) { it is ListSecretEnv },
            TaggedVariant<ApiCommand>("set_secret_env", SetSecretEnv.serializer()) { it is SetSecretEnv },
            TaggedVariant<ApiCommand>("delete_secret_env", DeleteSecretEnv.serializer()) { it is DeleteSecretEnv },
            TaggedVariant<ApiCommand>("get_remote_status", GetRemoteStatus.serializer()) { it is GetRemoteStatus },
            TaggedVariant<ApiCommand>("set_remote_settings", SetRemoteSettings.serializer()) { it is SetRemoteSettings },
            TaggedVariant<ApiCommand>("create_pairing_code", CreatePairingCode.serializer()) { it is CreatePairingCode },
            TaggedVariant<ApiCommand>("list_remote_devices", ListRemoteDevices.serializer()) { it is ListRemoteDevices },
            TaggedVariant<ApiCommand>("revoke_remote_device", RevokeRemoteDevice.serializer()) { it is RevokeRemoteDevice },
            TaggedVariant<ApiCommand>("list_clients", ListClients.serializer()) { it is ListClients },
        ),
        ::Unrecognized,
    )
}
