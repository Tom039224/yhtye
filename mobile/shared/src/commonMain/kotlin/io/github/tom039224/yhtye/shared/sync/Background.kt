package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.client.ConnectionState
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.transformLatest

// アプリが裏でも PC とつないでおくかの判断と、通知を出すかの判断 (android-client.md「段階 3」の通知と
// 「決定 (2026-10-11…)」の裏での接続)。Android 依存はここに置かない (Foreground Service・通知の実体は androidApp)。

/** 裏での接続の保ち方 (設定。端末に保存する)。 */
enum class BackgroundMode(val wire: String) {
    /** ペアリング中は常に PC とつなぎ続ける (既定)。 */
    Always("always"),

    /** オーケストレータのターンか終わっていないグループがある間だけつなぐ。ユーザーの番になって通知したらやめる。 */
    WorkOnly("work_only"),
    ;

    companion object {
        val DEFAULT: BackgroundMode = Always

        /** 保存された文字列 (無い・知らない値は既定)。 */
        fun parse(wire: String?): BackgroundMode = entries.find { it.wire == wire } ?: DEFAULT
    }
}

/**
 * いま PC で作業が動いているか (「作業中だけ」の間つなぎ続ける理由)。
 *
 * - 表示中のプロジェクト: オーケストレータ (かほかのセッション) のターン、running / merging のタスク、終わっていない
 *   グループ (`active` / `finishing`)、起きているオーケストレータ宛の未配達の受信箱。`merge_blocked` は走っていない
 *   (ユーザーが整えるまで止まる) ので含めない。
 * - 表示していないプロジェクト: 一覧の `open && running` (ターンか running / merging のタスク) だけ。イベントを畳み込んで
 *   いないので、グループの `active` だけで動いていない稀な状態は分からない。
 */
fun hasWork(app: AppState): Boolean {
    val view = app.project
    if (app.projects.any { it.id != view?.info?.id && it.open && it.running }) return true
    if (view == null) return false
    // 読み込み中は一覧の値で判断する
    if (view.phase != LoadPhase.Ready) return app.projects.any { it.id == view.info.id && it.open && it.running }
    if (hasRunningWork(view)) return true
    val state = view.state ?: return false
    if (state.groups.any { it.status == GroupStatus.Active || it.status == GroupStatus.Finishing }) return true
    // 起きているオーケストレータに渡す受信箱の項目が残っている (次のターンが始まる直前)
    return state.inbox.any { entry ->
        view.sessions.any { it.sessionKey == orchestratorKey(entry.chat) && it.status == SessionStatus.Live }
    }
}

/** [shouldKeepAlive] の材料。 */
data class KeepAliveInput(
    val paired: Boolean,
    val mode: BackgroundMode,
    val connection: ConnectionState?,
    val hasWork: Boolean,
)

/**
 * 裏でも接続を保つか (= Foreground Service を動かすか)。ペアリング中で、失効 (401)・拒否 (403) でなく
 * (再接続がもう働かないので)、「常に」か「作業中だけ」で作業がある。
 */
fun shouldKeepAlive(i: KeepAliveInput): Boolean {
    if (!i.paired) return false
    if (i.connection is ConnectionState.Revoked || i.connection is ConnectionState.Rejected) return false
    return when (i.mode) {
        BackgroundMode.Always -> true
        BackgroundMode.WorkOnly -> i.hasWork
    }
}

/** 保ち続ける判断が false になってから実際に止めるまでの猶予 (ターンの切れ目・通知を出し終えるのを待つ)。 */
const val KEEP_ALIVE_GRACE_MS: Long = 3_000

/** true → false の変化だけ [graceMs] 遅らせる (その間に true に戻れば何も起きない)。false → true はすぐ。 */
@OptIn(ExperimentalCoroutinesApi::class)
fun Flow<Boolean>.holdOnFall(graceMs: Long = KEEP_ALIVE_GRACE_MS): Flow<Boolean> =
    // 同じ値の繰り返し (接続の状態が変わるたびに来る) で猶予の数え直しにならないよう、先に重複を除く
    distinctUntilChanged().transformLatest { keep ->
        if (!keep) delay(graceMs)
        emit(keep)
    }.distinctUntilChanged()

/** いまユーザーが見ている画面 (通知を出すかの判断に使う)。 */
data class DisplayContext(val foreground: Boolean, val project: String?, val chat: String?)

/** 通知を出すか: アプリが前面でそのチャットを表示中なら出さない (見えているので)。 */
fun shouldNotify(turn: UsersTurn, shown: DisplayContext): Boolean =
    !(shown.foreground && shown.project == turn.project && shown.chat == turn.chat)

/** 通知の文。 */
data class NotificationText(val title: String, val body: String?)

/**
 * `<チャットのタイトル> の作業が完了しました` と、オーケストレータの最後の発話の 1 行目。
 * 題の無いチャットは [untitled] (会話の上部と同じ「新しいチャット」)。
 */
fun usersTurnText(turn: UsersTurn, untitled: String): NotificationText = NotificationText(
    title = "${turn.title ?: untitled} の作業が完了しました",
    body = turn.lastMessage?.let(::firstLine)?.takeIf { it.isNotEmpty() },
)
