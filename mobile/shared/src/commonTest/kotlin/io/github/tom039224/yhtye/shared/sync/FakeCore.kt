package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentChoice
import io.github.tom039224.yhtye.shared.api.generated.AgentRole
import io.github.tom039224.yhtye.shared.api.generated.AgentSettings
import io.github.tom039224.yhtye.shared.api.generated.AgentSettingsLayer
import io.github.tom039224.yhtye.shared.api.generated.AgentSettingsView
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiError
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.AttachmentDelivery
import io.github.tom039224.yhtye.shared.api.generated.AttachmentKind
import io.github.tom039224.yhtye.shared.api.generated.AttachmentMeta
import io.github.tom039224.yhtye.shared.api.generated.BranchStatuses
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.ClientDevice
import io.github.tom039224.yhtye.shared.api.generated.ClientServer
import io.github.tom039224.yhtye.shared.api.generated.Clients
import io.github.tom039224.yhtye.shared.api.generated.EffortOption
import io.github.tom039224.yhtye.shared.api.generated.GitBranch
import io.github.tom039224.yhtye.shared.api.generated.GitOverview
import io.github.tom039224.yhtye.shared.api.generated.GitWorktree
import io.github.tom039224.yhtye.shared.api.generated.HarnessDetection
import io.github.tom039224.yhtye.shared.api.generated.HarnessModels
import io.github.tom039224.yhtye.shared.api.generated.LoggedEvent
import io.github.tom039224.yhtye.shared.api.generated.ModelEfforts
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.RoleSettings
import io.github.tom039224.yhtye.shared.api.generated.Snapshot
import io.github.tom039224.yhtye.shared.api.generated.UsageReport
import io.github.tom039224.yhtye.shared.client.ApiException
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.CoreConnection
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.yield
import kotlin.io.encoding.Base64
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

    /** プロジェクトの順 (id。新しく使った順)。`open_project` は使ったことにする。 */
    var projectOrder: List<String> = emptyList()

    /** プロジェクトのブランチの順 (キー。新しく使った順)。 */
    var branchOrder: List<String> = emptyList()

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
    /** `get_branch_statuses` の答え。 */
    var branchStatuses: BranchStatuses = BranchStatuses("main", emptyList())

    /** `ping` が返す PC の版 (空 = この項目を返す前の古い PC)。 */
    var hostVersion = ""

    /** `ping` が答える、PC が GUI 無しで動いているか。 */
    var hostHeadless = false

    /** ペアリング済みの端末 (`list_clients`)。 */
    var devices: List<ClientDevice> = emptyList()

    /** `open_chat` が起動中と答えるチャット。 */
    val starting = HashSet<String>()

    /** `create_branch` を失敗させる。 */
    var branchError: ApiError? = null

    /** エージェント設定の版 (`set_agent_settings` のたびに増える)。 */
    var agentRevision = 1_000L
    var agentSettingsSaves = 0

    /** 設定画面のコマンド用のメモリ上の設定 (null のものは既定の答え)。 */
    var agentView: AgentSettingsView? = null
    val harnessModels = HashMap<String, HarnessModels>()
    val modelEfforts = HashMap<String, List<EffortOption>>()
    var detections: List<HarnessDetection> = emptyList()
    val secrets = mutableListOf<String>()

    /** コアが保存している添付 (`stage_attachment` で増え、`discard_attachment` で減る)。 */
    val stored = LinkedHashMap<String, Pair<AttachmentMeta, ByteArray>>()

    /** `stage_attachment` が画像・テキストに予測する送り方 (それ以外のファイルは常に `path`)。 */
    var predictedDelivery = AttachmentDelivery.Native

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

    /** `list_projects` の答え: [projectOrder] の順、載っていないものはその後に。 */
    private fun listing(): List<ProjectInfo> = sortByOrder(listOf(info) + others, projectOrder) { it.id }

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
        ApiCommand.Ping -> ApiResponse.Pong(host, hostVersion, hostHeadless)
        ApiCommand.ListClients -> ApiResponse.Clients(Clients(ClientServer(host), devices))
        ApiCommand.ListProjects -> ApiResponse.Projects(listing())
        is ApiCommand.OpenProject -> {
            val project = (others.find { it.path == command.path } ?: info).copy(open = true)
            projectOrder = listOf(project.id) + projectOrder.filter { it != project.id }
            ApiResponse.Project(project)
        }
        is ApiCommand.ReorderProjects -> {
            val known = listing().map { it.id }
            val ids = command.ids.filter { it in known }.distinct()
            projectOrder = ids + projectOrder.filter { it !in ids }
            ApiResponse.Projects(listing())
        }
        is ApiCommand.GetBranchOrder -> ApiResponse.BranchOrder(branchOrder)
        is ApiCommand.ReorderBranches -> {
            val keys = command.keys.filter { it.isNotEmpty() }.distinct()
            branchOrder = keys + branchOrder.filter { it !in keys }
            ApiResponse.BranchOrder(branchOrder)
        }
        is ApiCommand.GetSnapshot -> ApiResponse.Snapshot(snapshot)
        is ApiCommand.ListEvents -> {
            val limit = minOf(command.limit ?: 500, pageCap)
            val after = log.filter { !it.live && it.seq > command.afterSeq }
            val events = after.take(limit.toInt()).map { LoggedEvent(it.seq, it.tsMs, it.project, false, it.body) }
            ApiResponse.Events(events, more = after.size > limit)
        }
        is ApiCommand.GetGitOverview -> ApiResponse.GitOverview(git)
        is ApiCommand.GetBranchStatuses -> ApiResponse.BranchStatuses(branchStatuses)
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
            agentView = saved(command)
            ApiResponse.AgentSettings(agentSettings(command.project))
        }
        is ApiCommand.ListHarnessModels ->
            ApiResponse.HarnessModels(harnessModels[command.harness] ?: refuse(ApiErrorCode.NotFound, "no harness ${command.harness}"))
        is ApiCommand.ListModelEfforts ->
            ApiResponse.ModelEfforts(ModelEfforts(command.harness, command.model, modelEfforts[modelKey(command.harness, command.model)].orEmpty()))
        ApiCommand.GetHarnesses, ApiCommand.DetectHarnesses -> ApiResponse.Harnesses(detections)
        is ApiCommand.SetHarnessPath -> {
            detections = detections.map {
                if (it.id == command.harness) it.copy(overridePath = command.path, installed = command.path != null || it.installed) else it
            }
            ApiResponse.Harnesses(detections)
        }
        is ApiCommand.StageAttachment -> {
            val data = try {
                Base64.decode(command.dataBase64)
            } catch (e: IllegalArgumentException) {
                refuse(ApiErrorCode.InvalidArgument, "data_base64 is not valid base64")
            }
            val mime = command.mime ?: "application/octet-stream"
            val kind = when {
                mime.startsWith("image/") -> AttachmentKind.Image
                mime.startsWith("text/") -> AttachmentKind.Text
                else -> AttachmentKind.File
            }
            val delivery = if (kind == AttachmentKind.File) AttachmentDelivery.Path else predictedDelivery
            val meta = AttachmentMeta("att-${stored.size + 1}", command.name, mime, data.size.toLong(), kind, delivery)
            stored[meta.id] = meta to data
            ApiResponse.AttachmentStaged(meta)
        }
        is ApiCommand.DiscardAttachment -> {
            stored.remove(command.id) ?: refuse(ApiErrorCode.NotFound, "no attachment ${command.id}")
            ApiResponse.Accepted
        }
        is ApiCommand.GetAttachment -> {
            val (meta, data) = stored[command.id] ?: refuse(ApiErrorCode.NotFound, "no attachment ${command.id}")
            ApiResponse.Attachment(meta, Base64.encode(data))
        }
        ApiCommand.ListSecretEnv -> ApiResponse.SecretEnv(secrets.toList())
        is ApiCommand.SetSecretEnv -> {
            if (command.name !in secrets) secrets += command.name
            ApiResponse.SecretEnv(secrets.toList())
        }
        is ApiCommand.DeleteSecretEnv -> {
            secrets -= command.name
            ApiResponse.SecretEnv(secrets.toList())
        }
        else -> ApiResponse.Accepted
    }

    /** 保存の後の設定: 全体か [command] のプロジェクトの層のその役割を置き換えた形。 */
    private fun saved(command: ApiCommand.SetAgentSettings): AgentSettingsView? {
        val current = agentView ?: return null
        fun AgentSettingsLayer.with(): AgentSettingsLayer = when (command.role) {
            AgentRole.Orchestrator -> copy(orchestrator = command.settings)
            AgentRole.Implementer -> copy(implementer = command.settings)
            AgentRole.Investigator -> copy(investigator = command.settings)
            AgentRole.Reviewer -> copy(reviewer = command.settings)
            AgentRole.Unrecognized -> this
        }
        val global = if (command.project == null) current.global.with() else current.global
        val layer = if (command.project != null) (current.projectLayer ?: AgentSettingsLayer()).with() else current.projectLayer
        fun pick(role: AgentRole, fallback: RoleSettings): RoleSettings =
            (layer?.of(role) ?: global.of(role)) ?: fallback
        val eff = current.effective
        return current.copy(
            global = global,
            projectLayer = layer,
            effective = AgentSettings(
                pick(AgentRole.Orchestrator, eff.orchestrator),
                pick(AgentRole.Implementer, eff.implementer),
                pick(AgentRole.Investigator, eff.investigator),
                pick(AgentRole.Reviewer, eff.reviewer),
            ),
        )
    }

    /** エージェント設定の最小の形 (中身は使わない。版だけを見る)。 */
    private fun agentSettings(project: String?): AgentSettingsView {
        agentView?.let { return it.copy(project = project, revision = agentRevision) }
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
