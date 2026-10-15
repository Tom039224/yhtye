package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.ApiJson
import io.github.tom039224.yhtye.shared.api.generated.AgentEvent
import io.github.tom039224.yhtye.shared.api.generated.AgentOutput
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.DomainConfig
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.GitBranch
import io.github.tom039224.yhtye.shared.api.generated.GitOverview
import io.github.tom039224.yhtye.shared.api.generated.GitWorktree
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.InboxEntry
import io.github.tom039224.yhtye.shared.api.generated.InboxItem
import io.github.tom039224.yhtye.shared.api.generated.InboxKind
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.ResultOfStringOrAgentError
import io.github.tom039224.yhtye.shared.api.generated.Role
import io.github.tom039224.yhtye.shared.api.generated.SessionRecord
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.api.generated.Snapshot
import io.github.tom039224.yhtye.shared.api.generated.State
import io.github.tom039224.yhtye.shared.api.generated.TextKind
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject

// PC のテストの道具 (`src/test/fixtures.ts` / `events.ts` / `chats.ts`) の移植。

/** リポジトリの `src/test/fixtures/<name>` (PC の vitest と同じ記録) の中身。 */
expect fun readFixture(name: String): String

/** 偽エージェントで本物のコアを動かして記録したイベント列 (crates/yhtye-core/tests/core_facade.rs)。 */
@Serializable
data class Recording(val start: Snapshot, val events: List<ApiEvent>, val end: Snapshot)

private fun recording(name: String): Recording = ApiJson.decodeFromString(Recording.serializer(), readFixture(name))

val FULL_RUN: Recording by lazy { recording("fake-run.json") }
val CANCEL_RUN: Recording by lazy { recording("fake-cancel.json") }

/** オーケストレータが落ち着いたグループを開けたままにし、Yhtye が催促してから終える (Stage 7a)。 */
val UNFINISHED_RUN: Recording by lazy { recording("fake-unfinished.json") }

val PROJECT = ProjectInfo(id = "repo", name = "repo", path = "/tmp/repo", open = true, running = false, awake = false)

fun durable(r: Recording): List<ApiEvent> = r.events.filter { !it.live }

/** 記録のチャット `C-1` のオーケストレータのセッションキー。 */
val ORCHESTRATOR: String = orchestratorKey("C-1")

const val TS_BASE: Long = 1_700_000_000_000

fun ev(seq: Long, body: ApiEventBody, live: Boolean = false, project: String = "repo"): ApiEvent =
    ApiEvent(seq, TS_BASE + seq, project, live, body)

private fun textUpdate(text: String): JsonElement = buildJsonObject {
    putJsonObject("content") {
        put("type", "text")
        put("text", text)
    }
}

fun chunk(seq: Long, session: String, text: String, thought: Boolean = false): ApiEvent {
    val out = if (thought) AgentOutput.ThoughtChunk(textUpdate(text)) else AgentOutput.MessageChunk(textUpdate(text))
    return ev(seq, ApiEventBody.Agent(session, AgentEvent.Output(out)), live = true)
}

fun agentText(seq: Long, session: String, text: String, kind: TextKind = TextKind.Message): ApiEvent =
    ev(seq, ApiEventBody.AgentText(session, kind, text))

fun turnEnded(seq: Long, session: String, reason: String = END_TURN): ApiEvent =
    ev(seq, ApiEventBody.Agent(session, AgentEvent.TurnEnded(ResultOfStringOrAgentError.Ok(reason))))

fun prompted(seq: Long, session: String, text: String = "p"): ApiEvent = ev(seq, ApiEventBody.Prompted(session, text))

fun domain(seq: Long, event: DomainEvent, project: String = "repo"): ApiEvent =
    ev(seq, ApiEventBody.Domain(event), project = project)

fun inbox(seq: Long, id: Long, chat: String, kind: InboxKind, text: String): ApiEvent =
    domain(seq, DomainEvent.InboxQueued(InboxEntry(id, chat, InboxItem(kind, emptyList(), text))))

fun userMessage(seq: Long, inboxId: Long, text: String, chat: String = "C-1"): ApiEvent =
    inbox(seq, inboxId, chat, InboxKind.UserMessage, text)

fun delivered(seq: Long, upTo: Long, chat: String = "C-1"): ApiEvent = domain(seq, DomainEvent.InboxDelivered(chat, upTo))

/** [session] の live の `usage_update` (ACP の `UsageUpdate`)。 */
fun usage(seq: Long, session: String, used: Long, size: Long, cost: Pair<Double, String>? = null): ApiEvent {
    val update = buildJsonObject {
        put("used", used)
        put("size", size)
        if (cost != null) {
            putJsonObject("cost") {
                put("amount", cost.first)
                put("currency", cost.second)
            }
        }
    }
    return ev(seq, ApiEventBody.Agent(session, AgentEvent.Output(AgentOutput.Usage(update))), live = true)
}

/** アプリ全体のイベント (コアが送る形: プロジェクトなし・seq 0・live)。 */
fun appWide(body: ApiEventBody): ApiEvent = ev(0, body, live = true, project = "")

fun sessionStarted(seq: Long, session: String, resumed: Boolean = false, role: Role = Role.Orchestrator): ApiEvent =
    ev(seq, ApiEventBody.SessionStarted(session, role, task = null, pid = 1, acpSessionId = "acp", resumed = resumed, agent = null))

fun sessionRecord(
    key: String,
    status: SessionStatus = SessionStatus.Live,
    turnRunning: Boolean = false,
    role: Role = Role.Orchestrator,
): SessionRecord = SessionRecord(key, role, task = null, acpSessionId = "acp", status = status, turnRunning = turnRunning)

// ---- 2 つのチャット (PC の `src/test/chats.ts`) ----------------------------------------------------

/** 固定の「今」(PC は Date.now())。 */
const val NOW: Long = 1_800_000_000_000

const val MAIN_WT: String = "/repo"
const val FEAT_WT: String = "/data/worktrees/repo/branches/feat-x"

/** C-1 (main のメインクローン、古い) と C-2 (feat/x の Yhtye の作業ツリー、最近使った)。 */
val CHAT_INFOS: List<ChatInfo> = listOf(
    ChatInfo("C-2", FEAT_WT, "Add feature x", createdMs = NOW - 7_200_000, lastUsedMs = NOW - 180_000),
    ChatInfo("C-1", MAIN_WT, "Refactor checkout", createdMs = NOW - 9_000_000, lastUsedMs = NOW - 3_600_000 * 5),
)

/** グループ G-1 (チャット C-1、タスク T-1、完了) と G-2 (チャット C-2、タスク T-2、進行中)。 */
fun chatState(): State {
    val base = FULL_RUN.end.state
    val t1 = base.tasks[0].copy(title = "checkout task", group = "G-1")
    val t2 = base.tasks[0].copy(id = "T-2", title = "feature task", group = "G-2")
    val g1 = base.groups[0].copy(id = "G-1", chat = "C-1", title = "Checkout group", baseBranch = "main", status = GroupStatus.Done)
    val g2 = g1.copy(id = "G-2", chat = "C-2", title = "Feature group", groupBranch = "yhtye/G-2", baseBranch = "feat/x", status = GroupStatus.Active)
    return emptyState("repo", DomainConfig(2)).copy(
        chats = CHAT_INFOS.map { io.github.tom039224.yhtye.shared.api.generated.Chat(it.id, it.worktree, it.title) }.reversed(),
        groups = listOf(g1, g2),
        tasks = listOf(t1, t2),
    )
}

/** チャットごとに 1 つの発言と返事 (seq 1-4)。 */
val CHAT_LOG: List<ApiEvent> = listOf(
    userMessage(1, 1, "Refactor checkout", "C-1"),
    userMessage(2, 2, "Add feature x", "C-2"),
    agentText(3, orchestratorKey("C-1"), "Reply about checkout"),
    agentText(4, orchestratorKey("C-2"), "Reply about feature"),
)

fun chatSnapshot(chats: List<ChatInfo> = CHAT_INFOS, state: State = chatState()): Snapshot =
    Snapshot(seq = CHAT_LOG.size.toLong(), state = state, sessions = emptyList(), chats = chats, starting = emptyList())

val CHAT_GIT: GitOverview = GitOverview(
    head = "main",
    headSha = "m".repeat(40),
    branches = listOf(
        GitBranch("main", "m".repeat(40)),
        GitBranch("feat/x", "f".repeat(40)),
        GitBranch("yhtye/G-2", "g".repeat(40)),
        GitBranch("yhtye/G-2-T-2", "t".repeat(40)),
    ),
    worktrees = listOf(
        GitWorktree(MAIN_WT, "main", "m".repeat(40), isMain = true, missing = false),
        GitWorktree(FEAT_WT, "feat/x", "f".repeat(40), isMain = false, missing = false),
        GitWorktree("/data/worktrees/repo/G-2/_group", "yhtye/G-2", "g".repeat(40), isMain = false, missing = false),
    ),
    commits = emptyList(),
    truncated = false,
)

/** JSON の文字列 (ツール呼び出しの引数など)。 */
fun jsonString(s: String): JsonElement = JsonPrimitive(s)
