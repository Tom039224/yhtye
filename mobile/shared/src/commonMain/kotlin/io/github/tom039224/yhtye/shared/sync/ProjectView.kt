package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentEvent
import io.github.tom039224.yhtye.shared.api.generated.AgentOutput
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.InboxKind
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.SessionRecord
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.api.generated.Snapshot
import io.github.tom039224.yhtye.shared.api.generated.State

// 表示中のプロジェクトの状態 (スナップショット + 畳み込んだドメインイベント)、セッション、セッションごとの会話と
// ストリーミング中の本文。純関数で、同期の手順は [AppStore] にある。PC の `src/store/project.ts` の移植。
//
// 規則 (core-design.md §2): durable イベントは `seq` の順に 1 度だけ適用する ([ProjectView.cursor] が最後に適用したもの)。
// ドメインとセッションのイベントはスナップショットより新しいもの ([ProjectView.stateSeq] より後) だけ状態に畳み込み、
// 会話はログ全体から作る。live のイベントはストリーミングの表示 (と使用量・起動中) にだけ使う。

/** プロジェクトの読み込みの段階。 */
enum class LoadPhase { Loading, Ready, Error }

data class ProjectView(
    val info: ProjectInfo,
    val phase: LoadPhase = LoadPhase.Loading,
    val loadError: String? = null,
    /** 状態の元にしたスナップショットの `seq`。 */
    val stateSeq: Long = 0,
    /** 最後に適用した durable イベントの `seq`。 */
    val cursor: Long = 0,
    /** 会話に入っている最も古いイベントの `seq` (古い履歴は求めに応じて前に読む。1 = ログ全体を読んだ)。 */
    val historyStart: Long = 1,
    /** 古い履歴のページを読んでいる。 */
    val loadingOlder: Boolean = false,
    /** 古い履歴の読み込みが失敗した理由 (次の読み込みの開始で消える)。 */
    val olderError: String? = null,
    val state: State? = null,
    /** プロジェクトのチャット (`Snapshot.chats` から、以後はイベントで畳み込む)。 */
    val chats: List<ChatInfo> = emptyList(),
    /** 会話とグループを出すチャット (null: プロジェクトにチャットが無い)。 */
    val selectedChat: String? = null,
    /** 別のチャットを表示している間に通知が届いたチャット (未読の印。選ぶと消える)。 */
    val unread: Set<String> = emptySet(),
    /**
     * コアがオーケストレータを起動しているチャット (このクライアントか他のクライアントが開いた・送った:
     * `open_chat` の応答、`session_starting`、スナップショットの `starting`)。セッションが上がるか止まるか、
     * 起動を諦めたら消える。
     */
    val resuming: Set<String> = emptySet(),
    val sessions: List<SessionRecord> = emptyList(),
    val transcripts: Transcripts = emptyMap(),
    val streaming: Streaming = emptyMap(),
    /** 各セッションが最後に知らせた文脈の使用量 (live の `usage_update`。保存されないので読み直すと空)。 */
    val contextUsage: Map<String, ContextUsage> = emptyMap(),
    /** ユーザーが頼んだ `/compact` のターン中のセッション (ログから畳み込む)。 */
    val compacting: Set<String> = emptySet(),
)

fun newProjectView(info: ProjectInfo): ProjectView = ProjectView(info)

/**
 * スナップショットから始める。会話にはスナップショットまでの最後の [window] 件だけを読む (状態には要らない)。
 * それより古い履歴はユーザーが求めたら [prependHistory] で前に足す。表示するチャットは [remembered] があればそれ、
 * 無ければ最近使ったもの。
 */
fun applySnapshot(view: ProjectView, snap: Snapshot, window: Long = Long.MAX_VALUE, remembered: String? = null): ProjectView {
    val historyStart = maxOf(1L, snap.seq - window + 1)
    return view.copy(
        state = snap.state,
        chats = snap.chats,
        selectedChat = initialChat(snap.chats, remembered),
        unread = emptySet(),
        resuming = snap.starting.filter(::isOrchestratorKey).mapTo(LinkedHashSet(), ::chatOfOrchestratorKey),
        sessions = snap.sessions,
        stateSeq = snap.seq,
        cursor = historyStart - 1,
        historyStart = historyStart,
    )
}

/** 保存済みのイベント `[from, historyStart)` (古い順) を会話の前に足す。 */
fun prependHistory(view: ProjectView, events: List<ApiEvent>, from: Long): ProjectView {
    val page = events.filter { it.seq >= from && it.seq < view.historyStart }
    val older = page.fold(emptyMap<String, List<TranscriptItem>>()) { t, e -> applyTranscriptEvent(t, e, view.state) }
    return view.copy(transcripts = prependTranscripts(older, view.transcripts), historyStart = from)
}

/** 次の durable イベントを適用する (呼ぶ側が `seq == cursor + 1` を保証する)。 */
fun applyDurable(view: ProjectView, ev: ApiEvent): ProjectView {
    val next = view.copy(
        cursor = ev.seq,
        transcripts = applyTranscriptEvent(view.transcripts, ev, view.state),
        streaming = settleStreaming(view.streaming, ev),
        compacting = applyCompacting(view.compacting, ev.body),
        contextUsage = forgetUsage(view.contextUsage, ev.body),
    )
    if (ev.seq <= view.stateSeq) return next
    val body = ev.body
    val chats = applyChatEvent(next.chats, ev)
    val state = next.state
    if (body is ApiEventBody.Domain && state != null) {
        val applied = next.copy(
            chats = chats,
            unread = markUnread(next, body.event),
            state = applyDomainEvent(state, body.event),
        )
        val event = body.event
        if (event is DomainEvent.ChatDeleted) return forgetChat(applied, state, event.chat)
        // チャットの無かったプロジェクトは、最初に作られたチャットを出す (他のクライアントが作ったものも)
        return if (applied.selectedChat == null && chats.isNotEmpty()) applied.copy(selectedChat = initialChat(chats, null)) else applied
    }
    return next.copy(
        chats = chats,
        resuming = settleResuming(next.resuming, body),
        sessions = applySessionEvent(next.sessions, body),
    )
}

/**
 * オーケストレータのセッションが上がる・止まる、または起動を諦めたら、そのチャットは起動中でなくなる。
 * 続きのある失敗 (`retrying`: 復元の再試行や新しいセッションへの切り替え) はまだ起動中。
 */
private fun settleResuming(resuming: Set<String>, body: ApiEventBody): Set<String> {
    val session = when (body) {
        is ApiEventBody.SessionStarted -> body.session
        is ApiEventBody.SessionStopped -> body.session
        is ApiEventBody.SessionFailed -> body.session.takeIf { !body.retrying }
        else -> null
    } ?: return resuming
    if (!isOrchestratorKey(session)) return resuming
    val chat = chatOfOrchestratorKey(session)
    return if (chat in resuming) resuming - chat else resuming
}

/**
 * コアが [chat] のオーケストレータを起動しているかを記録する (`open_chat` の応答)。すでに上がっている
 * セッション (応答より先にイベントが来た) は起動中にしない。
 */
fun markResuming(view: ProjectView, chat: String, starting: Boolean): ProjectView {
    val on = starting && orchestratorSession(view, chat)?.status != SessionStatus.Live
    if ((chat in view.resuming) == on) return view
    return view.copy(resuming = if (on) view.resuming + chat else view.resuming - chat)
}

/** `/compact` のプロンプトで圧縮が始まり、そのターンの終わり (か別のプロンプト) で終わる。 */
private fun applyCompacting(compacting: Set<String>, body: ApiEventBody): Set<String> = when (body) {
    is ApiEventBody.Prompted -> if (body.text == COMPACT_PROMPT) compacting + body.session else compacting - body.session
    is ApiEventBody.SessionStarted -> compacting - body.session
    is ApiEventBody.SessionStopped -> compacting - body.session
    is ApiEventBody.Agent ->
        if (body.event is AgentEvent.TurnEnded || body.event is AgentEvent.Exited) compacting - body.session else compacting
    else -> compacting
}

/** 新しい (復元でない) セッションは空の文脈で始まる: 前の使用量は古い。 */
private fun forgetUsage(usage: Map<String, ContextUsage>, body: ApiEventBody): Map<String, ContextUsage> =
    if (body is ApiEventBody.SessionStarted && !body.resumed) usage - body.session else usage

/**
 * 削除されたチャットが残したものを消す: 会話、そのエージェントのセッションと本文、未読の印。表示中なら別の
 * チャットを出す。[before] はまだそのグループとタスクを持っている状態。冪等 (コアの応答とイベントの両方が来る)。
 */
fun forgetChat(view: ProjectView, before: State?, chat: String): ProjectView {
    val groups = before?.groups.orEmpty().filter { it.chat == chat }.mapTo(HashSet()) { it.id }
    val tasks = before?.tasks.orEmpty().filter { it.group in groups }.map { it.id }
    val key = orchestratorKey(chat)
    val ofChat = { session: String -> session == key || tasks.any { session.startsWith("$it/") } }
    return view.copy(
        selectedChat = if (view.selectedChat == chat) initialChat(view.chats, null) else view.selectedChat,
        unread = view.unread - chat,
        resuming = view.resuming - chat,
        sessions = view.sessions.filterNot { ofChat(it.sessionKey) },
        transcripts = view.transcripts.filterKeys { !ofChat(it) },
        streaming = view.streaming.filterKeys { !ofChat(it) },
        contextUsage = view.contextUsage.filterKeys { !ofChat(it) },
        compacting = view.compacting.filterNot(ofChat).toSet(),
    )
}

/** コアが削除したチャットを、そのイベントが届く前に一覧と画面から外す。 */
fun removeChatNow(view: ProjectView, chat: String): ProjectView =
    forgetChat(view.copy(chats = view.chats.filter { it.id != chat }), view.state, chat)

/** 表示していないチャットへの通知はそのチャットに未読の印を付ける (選ぶと消える)。 */
private fun markUnread(view: ProjectView, event: DomainEvent): Set<String> {
    if (event !is DomainEvent.InboxQueued) return view.unread
    val chat = event.entry.chat
    if (event.entry.item.kind == InboxKind.UserMessage || chat == view.selectedChat || chat in view.unread) return view.unread
    return view.unread + chat
}

/**
 * live のイベント: 本文の断片はストリーミングに、使用量の更新はセッションごとに持つ。それ以外の live
 * (保存に失敗したときコアは live で流す) は表示するが状態には畳み込まない。
 */
fun applyLive(view: ProjectView, ev: ApiEvent): ProjectView {
    val streaming = applyChunk(view.streaming, ev)
    if (streaming != null) return if (streaming == view.streaming) view else view.copy(streaming = streaming)
    val body = ev.body
    if (body is ApiEventBody.SessionStarting) {
        // 頼んだのが他のクライアントでも: チャットは起動中と出す
        return if (isOrchestratorKey(body.session)) markResuming(view, chatOfOrchestratorKey(body.session), true) else view
    }
    if (body is ApiEventBody.Agent) {
        val out = (body.event as? AgentEvent.Output)?.data
        if (out is AgentOutput.Usage) {
            val usage = usageInfo(out.update) ?: return view
            return view.copy(contextUsage = view.contextUsage + (body.session to usage))
        }
    }
    return view.copy(transcripts = applyTranscriptEvent(view.transcripts, ev, view.state))
}

/** ストリーミング途中の本文を捨てる (切断で断片は失われる)。 */
fun clearStreaming(view: ProjectView): ProjectView = if (view.streaming.isEmpty()) view else view.copy(streaming = emptyMap())

/** チャットのオーケストレータのセッション (起動したことがあれば)。 */
fun orchestratorSession(view: ProjectView, chat: String?): SessionRecord? =
    chat?.let { c -> view.sessions.find { it.sessionKey == orchestratorKey(c) } }

/** 選択中のチャットの一覧の項目。 */
fun selectedChatInfo(view: ProjectView): ChatInfo? = view.chats.find { it.id == view.selectedChat }
