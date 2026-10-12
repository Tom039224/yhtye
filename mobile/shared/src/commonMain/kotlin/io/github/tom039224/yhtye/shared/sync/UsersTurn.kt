package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.SessionRecord
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.api.generated.State
import io.github.tom039224.yhtye.shared.api.generated.TextKind

/**
 * オーケストレータの手が止まり、ユーザーの番になった (android-client.md「段階 3」の通知の条件)。
 * 通知を出すかどうか (前面でこのチャットを表示中なら出さない) は受け取る側が決める。
 */
data class UsersTurn(
    val project: String,
    val chat: String,
    /** チャットの題 (まだ無ければ null)。 */
    val title: String?,
    /** このターンでオーケストレータが最後に言ったこと (無ければ null)。通知はその 1 行目を出す。 */
    val lastMessage: String?,
    /** ターンが終わった時刻 (エポックからのミリ秒)。 */
    val tsMs: Long,
)

/**
 * ターンを正常に終えた [chat] について、ユーザーの番か: 受信箱が空 (オーケストレータを起こすものが無い) で、
 * 走っているグループ (`active` / `finishing`) が無く、オーケストレータが次のターンを始めていない。
 * `merge_blocked` のグループは走っていない (ユーザーが整えるまで止まる) のでユーザーの番に含める。
 */
fun isUsersTurn(state: State, sessions: List<SessionRecord>, chat: String): Boolean {
    if (state.chats.none { it.id == chat }) return false
    if (state.inbox.any { it.chat == chat }) return false
    if (state.groups.any { it.chat == chat && (it.status == GroupStatus.Active || it.status == GroupStatus.Finishing) }) return false
    val orchestrator = sessions.find { it.sessionKey == orchestratorKey(chat) }
    return !(orchestrator?.status == SessionStatus.Live && orchestrator.turnRunning)
}

/**
 * 表示中のプロジェクトで [ev] (適用済み) がユーザーの番を作ったか。[before] は適用する前、[after] は適用した後のビュー。
 * スナップショットより古いイベント (履歴の読み込み) と、ユーザーが頼んだ `/compact` のターンの終わりは数えない。
 */
fun usersTurnAfter(before: ProjectView, after: ProjectView, ev: ApiEvent): UsersTurn? {
    if (ev.live || ev.seq <= before.stateSeq) return null
    val chat = chatOfNormalTurnEnd(ev.body) ?: return null
    val key = orchestratorKey(chat)
    if (key in before.compacting) return null
    val state = after.state ?: return null
    if (!isUsersTurn(state, after.sessions, chat)) return null
    val said = lastSaid(after.transcripts[key].orEmpty())
    return UsersTurn(after.info.id, chat, after.chats.find { it.id == chat }?.title, said, ev.tsMs)
}

/** 会話の最後のターン (最後のユーザーの発言・通知より後) でオーケストレータが最後に言ったこと。 */
private fun lastSaid(items: List<TranscriptItem>): String? {
    for (item in items.asReversed()) {
        when (item) {
            is TranscriptItem.Text -> if (item.textKind == TextKind.Message) return item.text
            is TranscriptItem.User, is TranscriptItem.Notice, is TranscriptItem.Compact -> return null
            else -> Unit
        }
    }
    return null
}

/**
 * 表示していないプロジェクトの、ユーザーの番の手がかり。表示していないプロジェクトのイベントは畳み込まないので、
 * オーケストレータの最後の発話と `/compact` のターンかどうかだけを生のイベントから覚えておき、正常なターンの
 * 終わりを見たらスナップショットで条件を確かめる ([AppStore] がする)。
 */
internal class BackgroundTurns {
    private val lastMessage = HashMap<Pair<String, String>, String>()
    private val compacting = HashSet<Pair<String, String>>()

    /** [ev] を覚え、正常なターンの終わりならそのチャット (確かめる対象) を返す。 */
    fun observe(ev: ApiEvent): String? {
        if (ev.live) return null
        when (val body = ev.body) {
            is ApiEventBody.AgentText -> if (body.kind == TextKind.Message && isOrchestratorKey(body.session)) {
                lastMessage[ev.project to body.session] = body.text
            }
            is ApiEventBody.Prompted -> if (isOrchestratorKey(body.session)) {
                val key = ev.project to body.session
                // 新しいターン: 前のターンの発話は忘れる
                lastMessage.remove(key)
                if (body.text == COMPACT_PROMPT) compacting += key else compacting -= key
            }
            else -> Unit
        }
        val chat = chatOfNormalTurnEnd(ev.body) ?: return null
        val wasCompacting = compacting.remove(ev.project to orchestratorKey(chat))
        return if (wasCompacting) null else chat
    }

    /** そのチャットのオーケストレータが最後に言ったこと。 */
    fun lastMessage(project: String, chat: String): String? = lastMessage[project to orchestratorKey(chat)]

    /** プロジェクトを表示し始めた (以後はビューから分かる) ので忘れる。 */
    fun forget(project: String) {
        lastMessage.keys.removeAll { it.first == project }
        compacting.removeAll { it.first == project }
    }
}
