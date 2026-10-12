package io.github.tom039224.yhtye.ui.conversation

import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.api.generated.State
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.ContextUsage
import io.github.tom039224.yhtye.shared.sync.GroupProgress
import io.github.tom039224.yhtye.shared.sync.LoadPhase
import io.github.tom039224.yhtye.shared.sync.Place
import io.github.tom039224.yhtye.shared.sync.ProjectView
import io.github.tom039224.yhtye.shared.sync.Streamed
import io.github.tom039224.yhtye.shared.sync.TranscriptItem
import io.github.tom039224.yhtye.shared.sync.awaitingFinish
import io.github.tom039224.yhtye.shared.sync.focusGroup
import io.github.tom039224.yhtye.shared.sync.groupProgress
import io.github.tom039224.yhtye.shared.sync.isOpenGroup
import io.github.tom039224.yhtye.shared.sync.orchestratorKey
import io.github.tom039224.yhtye.shared.sync.orchestratorSession
import io.github.tom039224.yhtye.shared.sync.placeLabel
import io.github.tom039224.yhtye.shared.sync.placeOf
import io.github.tom039224.yhtye.shared.sync.scopeState
import io.github.tom039224.yhtye.shared.sync.selectedChatInfo
import kotlin.math.roundToInt

// 中央の会話が出すもの。同期層の状態 ([AppState]) から導く純関数 (PC の `Conversation.tsx` の計算部分)。

/** PC と同じ文言。 */
const val NEW_CHAT_TITLE = "新しいチャット"
const val NO_CHAT_HINT = "サイドバーの「新しいチャット」から始めます"
const val NO_PROJECT_HINT = "プロジェクト (git リポジトリ) を開いてください。"
const val EMPTY_CONVERSATION = "会話はまだありません"

/** オーケストレータの状態 (PC の SessionBadge)。 */
sealed interface SessionBadge {
    /** 言葉 (読み上げとテスト)。 */
    val label: String

    data object Starting : SessionBadge {
        override val label = "starting"
    }

    /** 未起動、または Yhtye の再起動で止まっているだけ (中を抜いた丸)。 */
    data class Idle(override val label: String) : SessionBadge

    data object Ready : SessionBadge {
        override val label = "idle"
    }

    data object Working : SessionBadge {
        override val label = "working"
    }

    /** 止まった・失敗した (警告のアイコン)。 */
    data class Bad(override val label: String) : SessionBadge
}

/** 圧縮のボタンが押せるか (PC の `compactState`)。 */
sealed interface CompactState {
    data object Ready : CompactState

    data object Compacting : CompactState

    data class Disabled(val reason: String) : CompactState
}

/** 会話の末尾のグループ待機バナー (PC の WaitBanner)。 */
data class WaitBanner(val group: String, val text: String, val kind: Kind, val progress: GroupProgress) {
    enum class Kind { Waiting, Awaiting, Finishing }

    /** 完了したタスクの割合 (0〜100)。 */
    val share: Int get() = if (progress.total > 0) (progress.done * 100.0 / progress.total).roundToInt() else 0
}

data class ConversationModel(
    /** 表示中のプロジェクト (null: 開いていない)。 */
    val project: ProjectView?,
    val chat: ChatInfo?,
    val title: String,
    /** 作業ツリーのいまの場所 (git を読むまでは [Place.Unknown])。 */
    val place: Place,
    val items: List<TranscriptItem>,
    val streaming: Streamed?,
    /** 受信箱でまだ届いていないユーザーの発言 (受信箱の ID)。 */
    val queued: Set<Long>,
    val badge: SessionBadge,
    val turnRunning: Boolean,
    /** 送ったがオーケストレータのプロセスがまだ起動中 (遅延起動)。 */
    val starting: Boolean,
    val sending: Boolean,
    /** 送れない理由 (null: 送れる)。 */
    val disabledReason: String?,
    val compact: CompactState,
    val usage: ContextUsage?,
    val wait: WaitBanner?,
    /** グループのカードの中身 (タスクは今の状態から読む)。 */
    val state: State?,
) {
    val placeText: String? get() = placeLabel(place)

    /** 会話の上端より前に読んでいない履歴がある。 */
    val hasOlder: Boolean get() = (project?.historyStart ?: 1) > 1

    val isEmpty: Boolean get() = items.isEmpty() && streaming == null && !hasOlder
}

/** PC の SessionBadge の言葉と形。Yhtye の再起動で止まっているだけのセッションは問題として出さない。 */
private val PAUSED = setOf(SessionStatus.Suspended, SessionStatus.Interrupted)

fun conversationModel(app: AppState): ConversationModel {
    val view = app.project
    val chat = view?.let(::selectedChatInfo)
    val key = chat?.let { orchestratorKey(it.id) }
    val git = app.git?.takeIf { view != null && it.project == view.info.id }
    val place = if (chat == null) Place.Unknown else placeOf(chat, git?.overview, chat.id in git?.awaiting.orEmpty())
    val scoped = view?.state?.let { scopeState(it, chat?.id) }
    val queued = scoped?.inbox.orEmpty().mapTo(HashSet()) { it.id }
    val session = view?.let { orchestratorSession(it, chat?.id) }
    val resuming = chat != null && chat.id in view.resuming
    val turnRunning = session?.turnRunning ?: false
    val disabledReason = if (view == null || chat == null) NO_CHAT_HINT else when {
        app.connection !is ConnectionState.Connected -> "コアに接続していません"
        view.phase == LoadPhase.Loading -> "読み込み中…"
        view.phase == LoadPhase.Error -> "プロジェクトを読み込めませんでした"
        place == Place.Missing -> "作業ツリー「${chat.worktree}」が見つからないため送信できません (履歴は読めます)"
        else -> null
    }
    val compacting = app.busy.compacting || (key != null && key in view.compacting)
    return ConversationModel(
        project = view,
        chat = chat,
        title = chat?.let { it.title ?: NEW_CHAT_TITLE } ?: "orchestrator",
        place = place,
        items = key?.let { view.transcripts[it] }.orEmpty(),
        streaming = key?.let { view.streaming[it] }?.takeIf { it.message.isNotEmpty() || it.thought.isNotEmpty() },
        queued = queued,
        badge = sessionBadge(session?.status, turnRunning, resuming),
        turnRunning = turnRunning,
        starting = resuming || (queued.isNotEmpty() && session?.status != SessionStatus.Live),
        sending = app.busy.sending,
        disabledReason = disabledReason,
        compact = compactState(disabledReason, session?.status, turnRunning, compacting),
        usage = key?.let { view.contextUsage[it] },
        wait = scoped?.let(::waitBanner),
        state = view?.state,
    )
}

fun sessionBadge(status: SessionStatus?, running: Boolean, starting: Boolean): SessionBadge = when {
    starting -> SessionBadge.Starting
    status == null -> SessionBadge.Idle("not started")
    status == SessionStatus.Live -> if (running) SessionBadge.Working else SessionBadge.Ready
    status in PAUSED -> SessionBadge.Idle(status.wire)
    else -> SessionBadge.Bad(status.wire.ifEmpty { "unknown" })
}

/** 圧縮できるのは、起動していて手の空いたオーケストレータだけ (コアがそれ以外を断る)。理由はボタンの説明。 */
fun compactState(disabledReason: String?, status: SessionStatus?, turnRunning: Boolean, compacting: Boolean): CompactState = when {
    compacting -> CompactState.Compacting
    disabledReason != null -> CompactState.Disabled(disabledReason)
    status != SessionStatus.Live -> CompactState.Disabled("オーケストレータが起動していないため圧縮できません")
    turnRunning -> CompactState.Disabled("作業中は圧縮できません (ターン終了後に押せます)")
    else -> CompactState.Ready
}

/**
 * グループが動いている間、結果がオーケストレータに届くのは全タスクが落ち着いてから (設計 §3.4)。
 * そのことを会話の末尾で知らせる。文は PC の WaitBanner と同じ。
 */
fun waitBanner(state: State): WaitBanner? {
    val group = focusGroup(state)?.takeIf(::isOpenGroup) ?: return null
    val p = groupProgress(state, group.id)
    val finishing = group.status == GroupStatus.Finishing
    val awaiting = awaitingFinish(state, group)
    val text = when {
        finishing -> "グループ「${group.title}」を base ブランチへマージ中"
        awaiting -> "グループ「${group.title}」の全タスクが完了 — オーケストレータがグループを完了 (マージ) するのを待っています"
        else -> "グループ「${group.title}」の全タスク完了まで待機中 — ${p.done}/${p.total} 完了" +
            if (p.handling > 0) " · ${p.handling} 件 対処中" else ""
    }
    val kind = when {
        finishing -> WaitBanner.Kind.Finishing
        awaiting -> WaitBanner.Kind.Awaiting
        else -> WaitBanner.Kind.Waiting
    }
    return WaitBanner(group.id, text, kind, p)
}

private const val THOUSAND = 1_000.0
private const val MILLION = 1_000_000.0

/** `84k`、`1.2M`、`950` (PC の ContextBar の `tokens`)。 */
fun tokens(n: Double): String = when {
    n >= MILLION -> {
        val m = (n / MILLION * 10).roundToInt() / 10.0
        if (m % 1.0 == 0.0) "${m.toLong()}M" else "${m}M"
    }
    n >= THOUSAND -> "${(n / THOUSAND).roundToInt()}k"
    else -> "${n.roundToInt()}"
}

/** 文脈の使用の割合 (0〜100。大きさが分からなければ null)。 */
fun usagePercent(u: ContextUsage): Int? = if (u.size > 0) minOf(100, (u.used / u.size * 100).roundToInt()) else null

/** メーターの文 (`42% · 84k / 200k`。未取得は `—`)。 */
fun meterText(u: ContextUsage?): String {
    if (u == null) return "—"
    val pct = usagePercent(u)
    val amounts = "${tokens(u.used)} / ${if (u.size > 0) tokens(u.size) else "—"}"
    return if (pct == null) amounts else "$pct% · $amounts"
}
