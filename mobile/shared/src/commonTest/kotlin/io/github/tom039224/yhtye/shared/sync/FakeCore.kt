package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentChoice
import io.github.tom039224.yhtye.shared.api.generated.AgentSettings
import io.github.tom039224.yhtye.shared.api.generated.AgentSettingsLayer
import io.github.tom039224.yhtye.shared.api.generated.AgentSettingsView
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
import io.github.tom039224.yhtye.shared.api.generated.UsageReport
import io.github.tom039224.yhtye.shared.client.ApiException
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.CoreConnection
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.yield
import kotlin.reflect.KClass

/**
 * テスト用のメモリ上のコア (PC の `src/test/memoryTransport.ts` の `MemoryTransport` + `FakeCore`)。
 * 記録した・組み立てたイベントのログから `list_events` / `get_snapshot` / `open_project` に答える。
 */
class FakeCore(var info: ProjectInfo, var snapshot: Snapshot) : CoreConnection {
    override val state = MutableStateFlow<ConnectionState>(ConnectionState.Connected(1))
    override val events = MutableSharedFlow<ApiEvent>(extraBufferCapacity = 4096)

    /** 受けた要求 (順に)。 */
    val calls = mutableListOf<ApiCommand>()
    var log: List<ApiEvent> = emptyList()

    /** コアが知っている他のプロジェクト ([info] の後に並ぶ。`open_project` はパスで探す)。 */
    var others: List<ProjectInfo> = emptyList()

    /** 失敗させるコマンド (種類ごと)。 */
    val failures = HashMap<KClass<out ApiCommand>, ApiError>()

    /** 1 ページの上限 (クライアントが何件求めても)。 */
    var pageCap = Long.MAX_VALUE
    var git = GitOverview(
        head = "main",
        headSha = null,
        branches = listOf(GitBranch("main", "m".repeat(40))),
        worktrees = listOf(GitWorktree("/repo", "main", "m".repeat(40), isMain = true, missing = false)),
        commits = emptyList(),
        truncated = false,
    )
    var usage: UsageReport? = null
    var host = "test-host"

    /** `open_chat` が起動中と答えるチャット。 */
    val starting = HashSet<String>()

    /** `create_branch` を失敗させる。 */
    var branchError: ApiError? = null

    /** エージェント設定の版 (`set_agent_settings` のたびに増える)。 */
    var agentRevision = 1_000L
    var agentSettingsSaves = 0

    /** 答える前に呼ぶ (押し込むイベントを挟む・応答を止める)。 */
    var before: (suspend (ApiCommand) -> Unit)? = null

    init {
        // 記録はチャットを一時リポジトリに紐づけている: ここではそれがメインのクローン
        snapshot.chats.find { it.id == "C-1" }?.worktree?.let { recorded ->
            git = git.copy(worktrees = listOf(git.worktrees[0].copy(path = recorded)))
        }
    }

    fun emit(ev: ApiEvent) {
        check(events.tryEmit(ev))
    }

    inline fun <reified T : ApiCommand> callsOf(): List<T> = calls.filterIsInstance<T>()

    fun fail(type: KClass<out ApiCommand>, code: ApiErrorCode, message: String) {
        failures[type] = ApiError(code, message)
    }

    override suspend fun call(command: ApiCommand): ApiResponse = call(command, 0)

    override suspend fun call(command: ApiCommand, timeoutMs: Long): ApiResponse {
        calls += command
        yield() // 応答は後で届く (その間に他のコルーチンが動く)
        before?.invoke(command)
        failures[command::class]?.let { throw ApiException(it) }
        return handle(command)
    }

    private fun newChat(worktree: String): ChatInfo {
        val n = snapshot.chats.size + 1
        return ChatInfo("C-$n", worktree, null, createdMs = 1, lastUsedMs = 1)
    }

    /** [branch] の作業ツリー: チェックアウトされている所、無ければ新しい Yhtye のもの。 */
    private fun worktreeOf(branch: String): String {
        git.worktrees.find { it.branch == branch }?.let { return it.path }
        val path = "/data/worktrees/repo/branches/${branch.replace('/', '-')}"
        git = git.copy(worktrees = git.worktrees + GitWorktree(path, branch, "n".repeat(40), isMain = false, missing = false))
        return path
    }

    private fun refuse(code: ApiErrorCode, message: String): Nothing = throw ApiException(ApiError(code, message))

    private fun handle(command: ApiCommand): ApiResponse = when (command) {
        ApiCommand.Ping -> ApiResponse.Pong(host)
        ApiCommand.ListProjects -> ApiResponse.Projects(listOf(info) + others)
        is ApiCommand.OpenProject -> ApiResponse.Project((others.find { it.path == command.path } ?: info).copy(open = true))
        is ApiCommand.GetSnapshot -> ApiResponse.Snapshot(snapshot)
        is ApiCommand.ListEvents -> {
            val limit = minOf(command.limit ?: 500, pageCap)
            val after = log.filter { !it.live && it.seq > command.afterSeq }
            val events = after.take(limit.toInt()).map { LoggedEvent(it.seq, it.tsMs, it.project, false, it.body) }
            ApiResponse.Events(events, more = after.size > limit)
        }
        is ApiCommand.GetGitOverview -> ApiResponse.GitOverview(git)
        is ApiCommand.OpenChat ->
            if (snapshot.chats.none { it.id == command.chat }) {
                refuse(ApiErrorCode.NotFound, "no chat ${command.chat}")
            } else {
                ApiResponse.ChatOpened(command.chat in starting)
            }
        is ApiCommand.CreateChat -> ApiResponse.Chat(newChat(command.worktree ?: worktreeOf(command.branch.orEmpty())))
        is ApiCommand.RenameChat -> {
            val chat = snapshot.chats.find { it.id == command.chat } ?: refuse(ApiErrorCode.NotFound, "no chat ${command.chat}")
            val title = command.title.trim().replace(Regex("\\s+"), " ")
            if (title.isEmpty()) refuse(ApiErrorCode.InvalidArgument, "the title must not be empty")
            ApiResponse.Chat(chat.copy(title = title))
        }
        is ApiCommand.DeleteChat ->
            if (snapshot.chats.none { it.id == command.chat }) refuse(ApiErrorCode.NotFound, "no chat ${command.chat}") else ApiResponse.Accepted
        is ApiCommand.CreateBranch -> {
            branchError?.let { throw ApiException(it) }
            git = git.copy(branches = git.branches + GitBranch(command.name, "n".repeat(40)))
            ApiResponse.Chat(newChat(worktreeOf(command.name)))
        }
        is ApiCommand.GetUsage -> ApiResponse.Usage(usage ?: refuse(ApiErrorCode.Unavailable, "usage: no harness is configured to report usage"))
        is ApiCommand.GetAgentSettings -> ApiResponse.AgentSettings(agentSettings(command.project))
        is ApiCommand.SetAgentSettings -> {
            if (command.expectedRevision != null && command.expectedRevision != agentRevision) {
                refuse(ApiErrorCode.Conflict, "the agent settings were changed elsewhere since they were read; read them again")
            }
            agentRevision += 1
            agentSettingsSaves += 1
            ApiResponse.AgentSettings(agentSettings(command.project))
        }
        else -> ApiResponse.Accepted
    }

    /** エージェント設定の最小の形 (中身は使わない。版だけを見る)。 */
    private fun agentSettings(project: String?): AgentSettingsView {
        val builtin = AgentChoice("claude-code", "haiku")
        val empty = AgentSettingsLayer()
        val role = io.github.tom039224.yhtye.shared.api.generated.RoleSettings(emptyList(), builtin)
        return AgentSettingsView(
            harnesses = emptyList(),
            builtin = builtin,
            global = empty,
            project = project,
            projectLayer = project?.let { empty },
            effective = AgentSettings(role, role, role, role),
            revision = agentRevision,
        )
    }
}
