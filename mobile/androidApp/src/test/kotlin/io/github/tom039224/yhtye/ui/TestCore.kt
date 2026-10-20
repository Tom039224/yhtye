package io.github.tom039224.yhtye.ui

import io.github.tom039224.yhtye.shared.api.ApiJson
import io.github.tom039224.yhtye.shared.api.generated.AgentChoice
import io.github.tom039224.yhtye.shared.api.generated.AgentRole
import io.github.tom039224.yhtye.shared.api.generated.AgentSettings
import io.github.tom039224.yhtye.shared.api.generated.AgentSettingsLayer
import io.github.tom039224.yhtye.shared.api.generated.AgentSettingsView
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiError
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.AttachmentDelivery
import io.github.tom039224.yhtye.shared.api.generated.AttachmentKind
import io.github.tom039224.yhtye.shared.api.generated.AttachmentMeta
import io.github.tom039224.yhtye.shared.api.generated.BranchStatuses
import io.github.tom039224.yhtye.shared.api.generated.Candidate
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.ClientDevice
import io.github.tom039224.yhtye.shared.api.generated.ClientServer
import io.github.tom039224.yhtye.shared.api.generated.Clients
import io.github.tom039224.yhtye.shared.api.generated.DirectoryEntry
import io.github.tom039224.yhtye.shared.api.generated.DirectoryListing
import io.github.tom039224.yhtye.shared.api.generated.EffortOption
import io.github.tom039224.yhtye.shared.api.generated.GitBranch
import io.github.tom039224.yhtye.shared.api.generated.GitOverview
import io.github.tom039224.yhtye.shared.api.generated.GitWorktree
import io.github.tom039224.yhtye.shared.api.generated.HarnessDetection
import io.github.tom039224.yhtye.shared.api.generated.HarnessInfo
import io.github.tom039224.yhtye.shared.api.generated.HarnessModels
import io.github.tom039224.yhtye.shared.api.generated.LoggedEvent
import io.github.tom039224.yhtye.shared.api.generated.ModelEfforts
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.RoleSettings
import io.github.tom039224.yhtye.shared.api.generated.Snapshot
import io.github.tom039224.yhtye.shared.api.generated.StatsBucket
import io.github.tom039224.yhtye.shared.api.generated.StatsDay
import io.github.tom039224.yhtye.shared.api.generated.StatsDayHarness
import io.github.tom039224.yhtye.shared.api.generated.StatsProject
import io.github.tom039224.yhtye.shared.api.generated.StatsSource
import io.github.tom039224.yhtye.shared.api.generated.StatsTokens
import io.github.tom039224.yhtye.shared.api.generated.StatsTotals
import io.github.tom039224.yhtye.shared.api.generated.UsageStats
import io.github.tom039224.yhtye.shared.client.ApiException
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.CoreConnection
import io.github.tom039224.yhtye.shared.sync.of
import io.github.tom039224.yhtye.shared.sync.sortByOrder
import java.io.File
import java.time.LocalDate
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.io.encoding.Base64
import kotlin.reflect.KClass
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.jsonObject

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

/** アプリ全体のイベント (コアが送る形: プロジェクトなし・seq 0・live)。 */
fun appWide(body: ApiEventBody): ApiEvent = ApiEvent(0, 0, "", true, body)

val PROJECT = ProjectInfo(id = "repo", name = "repo", path = "/tmp/repo", open = true, running = false, awake = false)

/**
 * 画面のテスト用のコア (共通モジュールのテストの `FakeCore` の小さい版)。記録のスナップショットとログに答え、
 * 受けた要求を [calls] に残す。
 */
class TestCore(var snapshot: Snapshot, var log: List<ApiEvent>) : CoreConnection {
    override val state = kotlinx.coroutines.flow.MutableStateFlow<ConnectionState>(ConnectionState.Connected(1))
    override val events = kotlinx.coroutines.flow.MutableSharedFlow<ApiEvent>(extraBufferCapacity = 1024)

    val calls = CopyOnWriteArrayList<ApiCommand>()
    val failures = HashMap<KClass<out ApiCommand>, ApiError>()

    /** `ping` が返す PC の版 (空 = この項目を返す前の古い PC)。 */
    var hostVersion = ""

    /** `ping` が答える、PC が GUI 無しで動いているか。 */
    var hostHeadless = false

    /** `list_projects` の答え (`open_project` はパスが合うものを返す)。 */
    var projects: List<ProjectInfo> = listOf(PROJECT)

    /** プロジェクトの順 (id。新しく使った順)。`open_project` は使ったことにする。 */
    var projectOrder: List<String> = emptyList()

    /** プロジェクトのブランチの順 (キー。新しく使った順)。 */
    var branchOrder: List<String> = emptyList()

    val worktree = snapshot.chats.firstOrNull()?.worktree ?: "/tmp/repo"
    var git = GitOverview(
        head = "main",
        headSha = null,
        branches = listOf(GitBranch("main", "m".repeat(40))),
        worktrees = listOf(GitWorktree(worktree, "main", "m".repeat(40), isMain = true, missing = false)),
        commits = emptyList(),
        truncated = false,
    )

    /** `get_usage_stats` の答え (期間の日数・UTC からのずれ → 統計)。 */
    var usageStats: (days: Long, tzOffsetMin: Int) -> UsageStats = ::cannedUsageStats

    /** あれば `get_usage_stats` はこれが終わるまで答えない (読み込み中の画面を見るため)。 */
    var usageStatsGate: kotlinx.coroutines.CompletableDeferred<Unit>? = null

    /** `get_branch_statuses` の答え。 */
    var branchStatuses = BranchStatuses("main", emptyList())

    /** ペアリング済みの端末 (`list_clients`)。 */
    var devices: List<ClientDevice> = emptyList()

    /** `create_chat` / `create_branch` が作るチャットの ID。 */
    var nextChat = "C-9"

    /** コアが保存している添付 (`stage_attachment` で増え、`discard_attachment` で減る)。 */
    val attachments = LinkedHashMap<String, Pair<AttachmentMeta, ByteArray>>()

    // ---- 設定画面のコマンド用のメモリ上の設定 ----

    /** エージェント設定の版 (保存のたびに増える。他の端末の保存は [changeAgentSettingsElsewhere])。 */
    var agentRevision = 1_000L
    var agentSaves = 0
    var agentView: AgentSettingsView = defaultAgentView()
    val harnessModels = HashMap<String, HarnessModels>()
    val modelEfforts = HashMap<String, List<EffortOption>>()
    var detections: List<HarnessDetection> = emptyList()
    val secrets = mutableListOf<String>()

    /** 他の端末が保存した (版だけ進む。`conflict` や `settings_changed` の確認用)。 */
    fun changeAgentSettingsElsewhere(update: (AgentSettingsView) -> AgentSettingsView = { it }) {
        agentRevision += 1
        agentView = update(agentView)
    }

    inline fun <reified T : ApiCommand> callsOf(): List<T> = calls.filterIsInstance<T>()

    fun fail(type: KClass<out ApiCommand>, message: String) {
        failures[type] = ApiError(ApiErrorCode.InvalidState, message)
    }

    fun failWith(type: KClass<out ApiCommand>, code: ApiErrorCode, message: String) {
        failures[type] = ApiError(code, message)
    }

    fun emit(ev: ApiEvent) {
        check(events.tryEmit(ev))
    }

    override suspend fun call(command: ApiCommand): ApiResponse = call(command, 0)

    override suspend fun call(command: ApiCommand, timeoutMs: Long): ApiResponse {
        calls += command
        failures[command::class]?.let { throw ApiException(it) }
        return when (command) {
            ApiCommand.Ping -> ApiResponse.Pong("test-host", hostVersion, hostHeadless)
            ApiCommand.ListClients -> ApiResponse.Clients(Clients(ClientServer("test-host"), devices))
            ApiCommand.ListProjects -> ApiResponse.Projects(sortByOrder(projects, projectOrder) { it.id })
            is ApiCommand.OpenProject -> {
                val project = projects.find { it.path == command.path } ?: PROJECT
                projectOrder = listOf(project.id) + projectOrder.filter { it != project.id }
                ApiResponse.Project(project)
            }
            is ApiCommand.ReorderProjects -> {
                val ids = command.ids.filter { id -> projects.any { it.id == id } }.distinct()
                projectOrder = ids + projectOrder.filter { it !in ids }
                ApiResponse.Projects(sortByOrder(projects, projectOrder) { it.id })
            }
            is ApiCommand.GetBranchOrder -> ApiResponse.BranchOrder(branchOrder)
            is ApiCommand.ReorderBranches -> {
                val keys = command.keys.filter { it.isNotEmpty() }.distinct()
                branchOrder = keys + branchOrder.filter { it !in keys }
                ApiResponse.BranchOrder(branchOrder)
            }
            is ApiCommand.GetSnapshot -> ApiResponse.Snapshot(snapshot)
            is ApiCommand.ListEvents -> {
                val after = log.filter { !it.live && it.seq > command.afterSeq }
                val page = after.take((command.limit ?: 500).toInt())
                ApiResponse.Events(page.map { LoggedEvent(it.seq, it.tsMs, it.project, false, it.body) }, more = after.size > page.size)
            }
            is ApiCommand.GetGitOverview -> ApiResponse.GitOverview(git)
            is ApiCommand.GetBranchStatuses -> ApiResponse.BranchStatuses(branchStatuses)
            is ApiCommand.OpenChat -> ApiResponse.ChatOpened(false)
            is ApiCommand.RenameChat -> {
                val chat = snapshot.chats.first { it.id == command.chat }
                ApiResponse.Chat(chat.copy(title = command.title))
            }
            is ApiCommand.GetUsage -> throw ApiException(ApiError(ApiErrorCode.Unavailable, "no usage"))
            is ApiCommand.GetUsageStats -> {
                usageStatsGate?.await()
                // 待っている間に失敗を仕込まれたら、それを返す
                failures[command::class]?.let { throw ApiException(it) }
                ApiResponse.UsageStats(usageStats(command.days ?: 30, command.tzOffsetMin ?: 0))
            }
            is ApiCommand.ListDirectory -> ApiResponse.Directory(listDirectory(command.path, command.showHidden ?: false))
            is ApiCommand.GetAgentSettings -> ApiResponse.AgentSettings(settingsFor(command.project))
            is ApiCommand.SetAgentSettings -> {
                if (command.expectedRevision != null && command.expectedRevision != agentRevision) {
                    throw ApiException(ApiError(ApiErrorCode.Conflict, "the agent settings were changed elsewhere since they were read; read them again"))
                }
                agentRevision += 1
                agentSaves += 1
                agentView = saved(command)
                ApiResponse.AgentSettings(settingsFor(command.project))
            }
            is ApiCommand.ListHarnessModels ->
                ApiResponse.HarnessModels(harnessModels[command.harness] ?: throw ApiException(ApiError(ApiErrorCode.NotFound, "no harness ${command.harness}")))
            is ApiCommand.ListModelEfforts ->
                ApiResponse.ModelEfforts(ModelEfforts(command.harness, command.model, modelEfforts["${command.harness}/${command.model}"].orEmpty()))
            ApiCommand.GetHarnesses, ApiCommand.DetectHarnesses -> ApiResponse.Harnesses(detections)
            is ApiCommand.SetHarnessPath -> {
                detections = detections.map { if (it.id == command.harness) it.copy(overridePath = command.path) else it }
                ApiResponse.Harnesses(detections)
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
            is ApiCommand.StageAttachment -> {
                val data = Base64.decode(command.dataBase64)
                val mime = command.mime ?: "application/octet-stream"
                val kind = when {
                    mime.startsWith("image/") -> AttachmentKind.Image
                    mime.startsWith("text/") -> AttachmentKind.Text
                    else -> AttachmentKind.File
                }
                val delivery = if (kind == AttachmentKind.File) AttachmentDelivery.Path else AttachmentDelivery.Native
                val meta = AttachmentMeta("att-${attachments.size + 1}", command.name, mime, data.size.toLong(), kind, delivery)
                attachments[meta.id] = meta to data
                ApiResponse.AttachmentStaged(meta)
            }
            is ApiCommand.DiscardAttachment -> {
                attachments.remove(command.id) ?: throw ApiException(ApiError(ApiErrorCode.NotFound, "no attachment ${command.id}"))
                ApiResponse.Accepted
            }
            is ApiCommand.GetAttachment -> {
                val (meta, data) = attachments[command.id] ?: throw ApiException(ApiError(ApiErrorCode.NotFound, "no attachment ${command.id}"))
                ApiResponse.Attachment(meta, Base64.encode(data))
            }
            is ApiCommand.CreateChat ->
                ApiResponse.Chat(ChatInfo(nextChat, command.worktree ?: "/tmp/wt/${command.branch}", null, createdMs = 0, lastUsedMs = 0))
            is ApiCommand.CreateBranch -> ApiResponse.Chat(ChatInfo(nextChat, "/tmp/wt/${command.name}", null, createdMs = 0, lastUsedMs = 0))
            else -> ApiResponse.Accepted
        }
    }

    /** ホスト (ホームは `/home/u`) の小さなフォルダの木: `list_directory` の答え。 */
    private fun listDirectory(path: String?, showHidden: Boolean): DirectoryListing {
        val home = "/home/u"
        val dir = (if (path.isNullOrBlank() || path == "~") home else path).let { if (it.length > 1) it.trimEnd('/') else it }
        val folders = FS_TREE[dir] ?: throw ApiException(ApiError(ApiErrorCode.NotFound, "$dir does not exist"))
        return DirectoryListing(
            path = dir,
            parent = if (dir == "/") null else dir.substringBeforeLast('/').ifEmpty { "/" },
            home = home,
            isGitRepo = false,
            entries = folders
                .filter { showHidden || !it.startsWith(".") }
                .sortedBy { it.lowercase() }
                .map { DirectoryEntry(it, if (dir == "/") "/$it" else "$dir/$it", isGitRepo = it == "repo") },
        )
    }

    private fun settingsFor(project: String?): AgentSettingsView =
        agentView.copy(project = project, projectLayer = if (project == null) null else agentView.projectLayer ?: AgentSettingsLayer(), revision = agentRevision)

    private fun saved(command: ApiCommand.SetAgentSettings): AgentSettingsView {
        fun AgentSettingsLayer.with(): AgentSettingsLayer = when (command.role) {
            AgentRole.Orchestrator -> copy(orchestrator = command.settings)
            AgentRole.Implementer -> copy(implementer = command.settings)
            AgentRole.Investigator -> copy(investigator = command.settings)
            AgentRole.Reviewer -> copy(reviewer = command.settings)
            AgentRole.Unrecognized -> this
        }
        val view = agentView
        val global = if (command.project == null) view.global.with() else view.global
        val layer = if (command.project != null) (view.projectLayer ?: AgentSettingsLayer()).with() else view.projectLayer
        fun pick(role: AgentRole, fallback: RoleSettings): RoleSettings = layer.of(role) ?: global.of(role) ?: fallback
        val eff = view.effective
        return view.copy(
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
}

/** 設定画面のテストの初期の設定: 実装の役割だけ 2 行 (sonnet が既定)、あとは組み込みの既定。 */
fun defaultAgentView(): AgentSettingsView {
    val builtin = AgentChoice("claude-code", "haiku")
    val base = RoleSettings(listOf(Candidate("claude-code", "haiku", null, "")), builtin)
    val impl = RoleSettings(
        listOf(Candidate("claude-code", "sonnet", "medium", "ふつうの変更"), Candidate("claude-code", "opus", "high", "")),
        AgentChoice("claude-code", "sonnet", "medium"),
    )
    return AgentSettingsView(
        harnesses = listOf(HarnessInfo("claude-code", "Claude Code", false), HarnessInfo("codex", "Codex", false)),
        builtin = builtin,
        global = AgentSettingsLayer(implementer = impl),
        effective = AgentSettings(base, impl, base, base),
        revision = 1_000L,
    )
}

/** `list_directory` の偽のホストのフォルダ: パス → 直下のフォルダの名前。 */
private val FS_TREE: Map<String, List<String>> = mapOf(
    "/" to listOf("home", "tmp"),
    "/home" to listOf("u"),
    "/home/u" to listOf("Documents", "repo", ".config"),
    "/home/u/Documents" to emptyList(),
    "/home/u/repo" to listOf("src"),
    "/home/u/repo/src" to emptyList(),
    "/home/u/.config" to emptyList(),
    "/tmp" to emptyList(),
)

private fun tokens(input: Long, output: Long, cacheRead: Long, cacheCreation: Long) =
    StatsTokens(input, output, cacheRead, cacheCreation, input + output + cacheRead + cacheCreation)

/** 期間の最後の 2 日だけ Claude Code の使用がある小さな `get_usage_stats` の答え ([days] 日、最後の日は 2026-10-20)。 */
fun cannedUsageStats(days: Long, tzOffsetMin: Int): UsageStats {
    val usage = listOf(tokens(100, 200, 3000, 400), tokens(200, 400, 6000, 800))
    val sum = tokens(300, 600, 9000, 1200)
    return UsageStats(
        generatedAtMs = 1_792_000_000_000,
        periodDays = days,
        tzOffsetMin = tzOffsetMin,
        sources = listOf(
            StatsSource("claude-code", found = true, files = 2),
            StatsSource("codex", found = false, files = 0),
            StatsSource("opencode", found = false, files = 0),
        ),
        totals = StatsTotals(sum, sessions = 2),
        days = (0 until days).map { i ->
            val used = usage.getOrNull((i - (days - usage.size)).toInt())
            StatsDay(
                date = LocalDate.of(2026, 10, 20).minusDays(days - 1 - i).toString(),
                tokens = used ?: tokens(0, 0, 0, 0),
                sessions = if (used == null) 0 else 1,
                harnesses = if (used == null) emptyList() else listOf(StatsDayHarness("claude-code", used.total, 1)),
            )
        },
        byModel = listOf(StatsBucket("claude-opus-5-5", sum, sessions = 2, other = false)),
        byHarness = listOf(StatsBucket("claude-code", sum, sessions = 2, other = false)),
        byProject = listOf(StatsProject("repo", project = "repo", tokens = sum, sessions = 2, other = false)),
    )
}
