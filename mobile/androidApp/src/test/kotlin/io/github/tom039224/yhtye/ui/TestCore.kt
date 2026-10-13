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
import io.github.tom039224.yhtye.shared.api.generated.BranchStatuses
import io.github.tom039224.yhtye.shared.api.generated.Candidate
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.ClientDevice
import io.github.tom039224.yhtye.shared.api.generated.ClientServer
import io.github.tom039224.yhtye.shared.api.generated.Clients
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
import io.github.tom039224.yhtye.shared.client.ApiException
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.CoreConnection
import io.github.tom039224.yhtye.shared.sync.of
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

/** アプリ全体のイベント (コアが送る形: プロジェクトなし・seq 0・live)。 */
fun appWide(body: ApiEventBody): ApiEvent = ApiEvent(0, 0, "", true, body)

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

    /** `ping` が返す PC の版 (空 = この項目を返す前の古い PC)。 */
    var hostVersion = ""

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

    /** `get_branch_statuses` の答え。 */
    var branchStatuses = BranchStatuses("main", emptyList())

    /** ペアリング済みの端末 (`list_clients`)。 */
    var devices: List<ClientDevice> = emptyList()

    /** `create_chat` / `create_branch` が作るチャットの ID。 */
    var nextChat = "C-9"

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
            ApiCommand.Ping -> ApiResponse.Pong("test-host", hostVersion)
            ApiCommand.ListClients -> ApiResponse.Clients(Clients(ClientServer("test-host"), devices))
            ApiCommand.ListProjects -> ApiResponse.Projects(projects)
            is ApiCommand.OpenProject -> ApiResponse.Project(projects.find { it.path == command.path } ?: PROJECT)
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
            is ApiCommand.CreateChat ->
                ApiResponse.Chat(ChatInfo(nextChat, command.worktree ?: "/tmp/wt/${command.branch}", null, createdMs = 0, lastUsedMs = 0))
            is ApiCommand.CreateBranch -> ApiResponse.Chat(ChatInfo(nextChat, "/tmp/wt/${command.name}", null, createdMs = 0, lastUsedMs = 0))
            else -> ApiResponse.Accepted
        }
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
