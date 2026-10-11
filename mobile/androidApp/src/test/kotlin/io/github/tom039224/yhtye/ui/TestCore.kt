package io.github.tom039224.yhtye.ui

import io.github.tom039224.yhtye.shared.api.ApiJson
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiError
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.GitBranch
import io.github.tom039224.yhtye.shared.api.generated.GitOverview
import io.github.tom039224.yhtye.shared.api.generated.GitWorktree
import io.github.tom039224.yhtye.shared.api.generated.LoggedEvent
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.Snapshot
import io.github.tom039224.yhtye.shared.client.ApiException
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.CoreConnection
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.jsonObject
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.reflect.KClass

/** 偽エージェントで本物のコアを動かした記録 (PC の vitest と同じ `src/test/fixtures/fake-run.json`)。 */
class Recording(val end: Snapshot, val events: List<ApiEvent>)

fun readRecording(name: String = "fake-run.json"): Recording {
    var dir: File? = File(System.getProperty("user.dir").orEmpty()).absoluteFile
    while (dir != null) {
        val file = File(dir, "src/test/fixtures/$name")
        if (file.isFile && File(dir, "mobile").isDirectory) {
            val json = ApiJson.parseToJsonElement(file.readText()).jsonObject
            return Recording(
                end = ApiJson.decodeFromJsonElement(Snapshot.serializer(), json.getValue("end")),
                events = ApiJson.decodeFromJsonElement(ListSerializer(ApiEvent.serializer()), json.getValue("events")),
            )
        }
        dir = dir.parentFile
    }
    error("src/test/fixtures/$name was not found")
}

val PROJECT = ProjectInfo(id = "repo", name = "repo", path = "/tmp/repo", open = true, running = false)

/**
 * 画面のテスト用のコア (共通モジュールのテストの `FakeCore` の小さい版)。記録のスナップショットとログに答え、
 * 受けた要求を [calls] に残す。
 */
class TestCore(var snapshot: Snapshot, var log: List<ApiEvent>) : CoreConnection {
    override val state = kotlinx.coroutines.flow.MutableStateFlow<ConnectionState>(ConnectionState.Connected(1))
    override val events = kotlinx.coroutines.flow.MutableSharedFlow<ApiEvent>(extraBufferCapacity = 1024)

    val calls = CopyOnWriteArrayList<ApiCommand>()
    val failures = HashMap<KClass<out ApiCommand>, ApiError>()

    /** `list_projects` の答え (`open_project` はパスが合うものを返す)。 */
    var projects: List<ProjectInfo> = listOf(PROJECT)

    val worktree = snapshot.chats.firstOrNull()?.worktree ?: "/tmp/repo"
    var git = GitOverview(
        head = "main",
        headSha = null,
        branches = listOf(GitBranch("main", "m".repeat(40))),
        worktrees = listOf(GitWorktree(worktree, "main", "m".repeat(40), isMain = true, missing = false)),
        commits = emptyList(),
        truncated = false,
    )

    /** `create_chat` / `create_branch` が作るチャットの ID。 */
    var nextChat = "C-9"

    inline fun <reified T : ApiCommand> callsOf(): List<T> = calls.filterIsInstance<T>()

    fun fail(type: KClass<out ApiCommand>, message: String) {
        failures[type] = ApiError(ApiErrorCode.InvalidState, message)
    }

    fun emit(ev: ApiEvent) {
        check(events.tryEmit(ev))
    }

    override suspend fun call(command: ApiCommand): ApiResponse = call(command, 0)

    override suspend fun call(command: ApiCommand, timeoutMs: Long): ApiResponse {
        calls += command
        failures[command::class]?.let { throw ApiException(it) }
        return when (command) {
            ApiCommand.Ping -> ApiResponse.Pong("test-host")
            ApiCommand.ListProjects -> ApiResponse.Projects(projects)
            is ApiCommand.OpenProject -> ApiResponse.Project(projects.find { it.path == command.path } ?: PROJECT)
            is ApiCommand.GetSnapshot -> ApiResponse.Snapshot(snapshot)
            is ApiCommand.ListEvents -> {
                val after = log.filter { !it.live && it.seq > command.afterSeq }
                val page = after.take((command.limit ?: 500).toInt())
                ApiResponse.Events(page.map { LoggedEvent(it.seq, it.tsMs, it.project, false, it.body) }, more = after.size > page.size)
            }
            is ApiCommand.GetGitOverview -> ApiResponse.GitOverview(git)
            is ApiCommand.OpenChat -> ApiResponse.ChatOpened(false)
            is ApiCommand.RenameChat -> {
                val chat = snapshot.chats.first { it.id == command.chat }
                ApiResponse.Chat(chat.copy(title = command.title))
            }
            is ApiCommand.GetUsage -> throw ApiException(ApiError(ApiErrorCode.Unavailable, "no usage"))
            is ApiCommand.CreateChat ->
                ApiResponse.Chat(ChatInfo(nextChat, command.worktree ?: "/tmp/wt/${command.branch}", null, createdMs = 0, lastUsedMs = 0))
            is ApiCommand.CreateBranch -> ApiResponse.Chat(ChatInfo(nextChat, "/tmp/wt/${command.name}", null, createdMs = 0, lastUsedMs = 0))
            else -> ApiResponse.Accepted
        }
    }
}

