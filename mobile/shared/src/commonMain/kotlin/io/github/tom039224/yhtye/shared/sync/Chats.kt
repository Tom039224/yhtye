package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.State
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

// プロジェクトのチャット (Stage 8): 一覧、選択、チャットごとに絞った状態。PC の `src/store/chats.ts` の移植。

/** Yhtye がグループ・タスク用に作るブランチ。その作業ツリーは作業場所として出さない。 */
const val INTERNAL_BRANCH_PREFIX: String = "yhtye/"
private const val ORCHESTRATOR_PREFIX = "orchestrator:"

/** ユーザーが付けられるチャットの題の長さの上限 (コアの `MAX_RENAMED_TITLE_CHARS`)。 */
const val MAX_CHAT_TITLE_CHARS: Int = 80

/** チャットのオーケストレータのセッションキー (`runtime::sessions`)。 */
fun orchestratorKey(chat: String): String = "$ORCHESTRATOR_PREFIX$chat"

fun isOrchestratorKey(session: String): Boolean = session.startsWith(ORCHESTRATOR_PREFIX)

/** このセッションキーを持つオーケストレータのチャット ([isOrchestratorKey] のとき)。 */
fun chatOfOrchestratorKey(session: String): String = session.removePrefix(ORCHESTRATOR_PREFIX)

fun isInternalBranch(name: String): Boolean = name.startsWith(INTERNAL_BRANCH_PREFIX)

/** 無ければ足す (応答とイベントの両方が同じチャットを運ぶ)。 */
fun upsertChat(chats: List<ChatInfo>, chat: ChatInfo): List<ChatInfo> =
    if (chats.any { it.id == chat.id }) chats else chats + chat

/** チャットの一覧を変える durable イベントを畳み込む (時刻は `ts_ms`)。変わらなければ同じリスト。 */
fun applyChatEvent(chats: List<ChatInfo>, ev: ApiEvent): List<ChatInfo> {
    val body = ev.body
    if (body is ApiEventBody.Prompted && isOrchestratorKey(body.session)) {
        val id = chatOfOrchestratorKey(body.session)
        if (chats.none { it.id == id }) return chats
        return chats.map { if (it.id == id) it.copy(lastUsedMs = ev.tsMs) else it }
    }
    if (body !is ApiEventBody.Domain) return chats
    return when (val event = body.event) {
        is DomainEvent.ChatCreated -> {
            val c = event.chat
            upsertChat(chats, ChatInfo(c.id, c.worktree, c.title, createdMs = ev.tsMs, lastUsedMs = ev.tsMs))
        }
        is DomainEvent.ChatTitled ->
            if (chats.none { it.id == event.chat }) chats else chats.map { if (it.id == event.chat) it.copy(title = event.title) else it }
        is DomainEvent.ChatDeleted ->
            if (chats.none { it.id == event.chat }) chats else chats.filter { it.id != event.chat }
        else -> chats
    }
}

/** 最近使った順 (同じなら新しいチャットが先)。 */
fun byRecency(chats: List<ChatInfo>): List<ChatInfo> =
    chats.sortedWith(compareByDescending<ChatInfo> { it.lastUsedMs }.thenByDescending { it.createdMs })

/** プロジェクトを開いたときに出すチャット: 覚えていたもの、無ければ最近使ったもの。 */
fun initialChat(chats: List<ChatInfo>, remembered: String?): String? {
    if (remembered != null && chats.any { it.id == remembered }) return remembered
    return byRecency(chats).firstOrNull()?.id
}

/** 1 つのチャットのグループ・そのタスクと help・受信箱だけに絞った [state]。 */
fun scopeState(state: State, chat: String?): State {
    val groups = state.groups.filter { it.chat == chat }
    val groupIds = groups.mapTo(HashSet()) { it.id }
    val tasks = state.tasks.filter { it.group in groupIds }
    val taskIds = tasks.mapTo(HashSet()) { it.id }
    return state.copy(
        groups = groups,
        tasks = tasks,
        helps = state.helps.filter { it.task in taskIds },
        inbox = state.inbox.filter { it.chat == chat },
    )
}

/** ユーザーのツール呼び出し (`cancel_group` / `cancel_task`) が関わるチャット。 */
fun chatOfUserCall(state: State?, args: JsonElement?): String? {
    if (state == null || args !is JsonObject) return null
    val taskId = (args["task_id"] as? JsonPrimitive)?.takeIf { it.isString }?.content
    val groupId = (args["group_id"] as? JsonPrimitive)?.takeIf { it.isString }?.content
    val group = groupId ?: taskId?.let { id -> state.tasks.find { it.id == id }?.group }
    return state.groups.find { it.id == group }?.chat
}
