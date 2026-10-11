package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.client.ConnectionState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

/** 通知の出し先 (Android は NotificationManager。テストは偽物)。 */
interface TurnNotifier {
    /** チャットごとに 1 つ (前の通知は置き換える)。 */
    fun post(turn: UsersTurn)

    /** そのチャットの通知を消す (チャットを見たので)。 */
    fun cancel(project: String, chat: String)
}

/**
 * 裏での接続と通知の判断をまとめる (ペアリングごとに 1 つ)。Foreground Service を動かすか ([keepAlive])、PC とつなぐか
 * ([wantConnected]) と、ユーザーの番を通知するか。Android 固有の実体 (サービス・通知) は外側が持つ。
 *
 * - [keepAlive]: [shouldKeepAlive]。「作業中だけ」で作業が終わったら、通知を出し終えるのを待つ猶予 ([graceMs]) の後に false。
 * - [wantConnected]: アプリが前面か、[keepAlive]。
 * - 通知: ユーザーの番 ([AppStore.usersTurns]) ごとに [shouldNotify] なら [notifier] へ。アプリが前面でそのチャットを表示している間は
 *   そのチャットの通知を消す。
 */
class BackgroundController(
    scope: CoroutineScope,
    mode: StateFlow<BackgroundMode>,
    connection: StateFlow<ConnectionState>,
    app: StateFlow<AppState>,
    foreground: StateFlow<Boolean>,
    usersTurns: Flow<UsersTurn>,
    private val notifier: TurnNotifier,
    graceMs: Long = KEEP_ALIVE_GRACE_MS,
) {
    val keepAlive: StateFlow<Boolean> = combine(mode, connection, app.map(::hasWork).distinctUntilChanged()) { m, c, w ->
        shouldKeepAlive(KeepAliveInput(true, m, c, w))
    }.holdOnFall(graceMs).stateIn(
        scope,
        SharingStarted.Eagerly,
        shouldKeepAlive(KeepAliveInput(true, mode.value, connection.value, hasWork(app.value))),
    )

    val wantConnected: StateFlow<Boolean> = combine(foreground, keepAlive) { f, k -> f || k }
        .stateIn(scope, SharingStarted.Eagerly, foreground.value || keepAlive.value)

    init {
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            usersTurns.collect { turn ->
                val shown = DisplayContext(foreground.value, app.value.project?.info?.id, app.value.project?.selectedChat)
                if (shouldNotify(turn, shown)) notifier.post(turn)
            }
        }
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            // 前面でチャットを見ている間は、そのチャットの通知は要らない
            combine(foreground, app.map { it.project?.info?.id to it.project?.selectedChat }.distinctUntilChanged()) { f, shown -> f to shown }
                .collect { (visible, shown) ->
                    val (project, chat) = shown
                    if (visible && project != null && chat != null) notifier.cancel(project, chat)
                }
        }
    }
}
