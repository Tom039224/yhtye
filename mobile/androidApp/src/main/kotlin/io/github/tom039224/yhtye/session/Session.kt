package io.github.tom039224.yhtye.session

import io.github.tom039224.yhtye.background.DeviceSettings
import io.github.tom039224.yhtye.background.OpenChatRequest
import io.github.tom039224.yhtye.background.SessionNotifications
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.KtorWsTransport
import io.github.tom039224.yhtye.shared.client.WsTransport
import io.github.tom039224.yhtye.shared.client.YhtyeClient
import io.github.tom039224.yhtye.shared.credentials.CredentialStore
import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.github.tom039224.yhtye.shared.pairing.InvalidPairingInput
import io.github.tom039224.yhtye.shared.pairing.InvalidPairingInputException
import io.github.tom039224.yhtye.shared.pairing.PairingClient
import io.github.tom039224.yhtye.shared.pairing.PairingResult
import io.github.tom039224.yhtye.shared.pairing.PairingTarget
import io.github.tom039224.yhtye.shared.pairing.parsePairingUri
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.BackgroundController
import io.github.tom039224.yhtye.shared.sync.ClientPrefs
import io.github.tom039224.yhtye.shared.sync.KEEP_ALIVE_GRACE_MS
import io.ktor.client.HttpClient
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

/**
 * アプリ全体の状態: ペアリングしているか、PC への接続 ([YhtyeClient]) とその同期層 ([AppStore])、受け取ったディープリンク、
 * 裏での接続 (常駐) と通知。アプリのプロセスに 1 つ (アクティビティから独立。画面が無くても同期層は動ける)。
 *
 * 接続するのは、アプリが前面にあるか、裏でも保つとき ([keepAlive]。常駐方式と作業の有無で決まる。Foreground Service が
 * これに従って動く)。ユーザーの番になったら通知を出す ([notifications])。
 *
 * @param scope メインスレッドのスコープ (状態の変更はここで順に行う)。
 * @param clientScope 通信 (受信した JSON の解析) と同期層を動かすスコープ。
 * @param clientPrefs 同期層のこの端末だけの設定 (最後のプロジェクト・表示していたチャット・下書き)。
 * @param device この端末だけの設定 (常駐方式など)。
 * @param transportFor PC への WebSocket (テストは偽物に替える)。
 */
class Session(
    private val store: CredentialStore,
    private val http: HttpClient,
    private val scope: CoroutineScope,
    private val clientScope: CoroutineScope,
    private val clientPrefs: ClientPrefs,
    val device: DeviceSettings,
    private val notifications: SessionNotifications,
    private val transportFor: (Credentials) -> WsTransport = { KtorWsTransport(http, it) },
    private val keepAliveGraceMs: Long = KEEP_ALIVE_GRACE_MS,
) {
    sealed interface Phase {
        data object Loading : Phase

        /** @property revoked PC で失効されたので戻ってきた (案内を出す)。 */
        data class Unpaired(val revoked: Boolean = false) : Phase

        /** @property sync PC の状態の写し (画面はここから読み、操作もここから送る)。 */
        data class Paired(val credentials: Credentials, val client: YhtyeClient, val sync: AppStore) : Phase
    }

    /** 外から届いたペアリングの指示 (ディープリンク)。画面が受け取ったら [consumeIncoming] する。 */
    sealed interface Incoming {
        data class Target(val target: PairingTarget) : Incoming

        data class Invalid(val reason: InvalidPairingInput) : Incoming
    }

    private val pairing = PairingClient(http)
    private val foreground = MutableStateFlow(false)

    /** 今の [YhtyeClient] の失効を待つコルーチン。 */
    private var revocationWatch: Job? = null

    /** 今のペアリングの常駐と通知の判断 ([BackgroundController]) を動かすスコープの Job。 */
    private var background: Job? = null

    private val _phase = MutableStateFlow<Phase>(Phase.Loading)
    val phase: StateFlow<Phase> = _phase.asStateFlow()

    private val _incoming = MutableStateFlow<Incoming?>(null)
    val incoming: StateFlow<Incoming?> = _incoming.asStateFlow()

    private val _keepAlive = MutableStateFlow<Boolean?>(null)

    /**
     * 裏でも PC とつなぎ続けるか (Foreground Service を動かすか)。null: まだ分からない (保存した認証情報を読んでいる間)。
     * ペアリングしていなければ false、している間は常駐方式で決まる (「作業中だけ」は作業が終わって通知を出した後に false)。
     */
    val keepAlive: StateFlow<Boolean?> = _keepAlive.asStateFlow()

    /** PC につながっている (常設の通知の文言)。 */
    @OptIn(ExperimentalCoroutinesApi::class)
    val connected: StateFlow<Boolean> = _phase
        .flatMapLatest { p -> if (p is Phase.Paired) p.client.state.map { it is ConnectionState.Connected } else flowOf(false) }
        .stateIn(scope, SharingStarted.Eagerly, false)

    private val _openChat = MutableStateFlow<OpenChatRequest?>(null)

    /** 通知のタップで開くよう頼まれたチャット (画面が開いたら [consumeOpenChat])。 */
    val openChat: StateFlow<OpenChatRequest?> = _openChat.asStateFlow()
    private var openChatSerial = 0L

    init {
        scope.launch {
            val stored = store.load()
            _phase.value = if (stored != null) paired(stored) else unpairedStart()
        }
    }

    private fun unpairedStart(): Phase.Unpaired {
        device.paired = false
        _keepAlive.value = false
        return Phase.Unpaired()
    }

    /** 通知のタップ: [project] の [chat] を開いてほしい。 */
    fun requestOpenChat(project: String, chat: String) {
        _openChat.value = OpenChatRequest(project, chat, ++openChatSerial)
    }

    fun consumeOpenChat(request: OpenChatRequest) {
        _openChat.compareAndSet(request, null)
    }

    /** PC にペアリングを申し込み、成功したら保存して接続を始める。 */
    suspend fun pair(target: PairingTarget, deviceName: String): PairingResult {
        val result = pairing.pair(target, deviceName)
        if (result is PairingResult.Paired) {
            store.save(result.credentials)
            _phase.value = paired(result.credentials)
        }
        return result
    }

    /** 端末内の認証情報を消してペアリング前に戻る (PC の端末一覧からは消えない)。 */
    fun unpair() {
        val current = _phase.value as? Phase.Paired ?: return
        revocationWatch?.cancel()
        stopBackground()
        current.sync.close()
        current.client.close()
        clientPrefs.clear()
        notifications.cancelAll()
        _phase.value = unpairedStart()
        scope.launch { store.clear() }
    }

    /** 失効の案内を閉じた。 */
    fun dismissRevoked() {
        if (_phase.value == Phase.Unpaired(revoked = true)) _phase.value = Phase.Unpaired()
    }

    /** `yhtye://pair?...` を受け取った。 */
    fun receiveLink(uri: String) {
        _incoming.value = parsePairingUri(uri).fold(
            onSuccess = { Incoming.Target(it) },
            onFailure = { Incoming.Invalid((it as InvalidPairingInputException).reason) },
        )
    }

    fun consumeIncoming() {
        _incoming.value = null
    }

    /** アプリが前面に来た: つなぐ (再接続待ちなら待たずに試す)。 */
    fun onForeground() {
        foreground.value = true
        val paired = _phase.value as? Phase.Paired ?: return
        paired.sync.setForeground(true)
        paired.client.start()
    }

    /**
     * アプリが裏に回った: 使用量と ping を止める。接続は、裏でも保つ判断 ([keepAlive]) でなければ切る (前面に戻ったら
     * つなぎ直して状態を取り直す)。保つなら同期層はそのまま動き続け、ユーザーの番になれば通知する。
     */
    fun onBackground() {
        foreground.value = false
        val paired = _phase.value as? Phase.Paired ?: return
        paired.sync.setForeground(false)
    }

    private fun paired(credentials: Credentials): Phase.Paired {
        val client = YhtyeClient(transportFor(credentials), clientScope)
        val sync = AppStore(client, clientScope, clientPrefs)
        sync.setForeground(foreground.value)
        sync.start()
        revocationWatch?.cancel()
        revocationWatch = scope.launch {
            client.state.first { it == ConnectionState.Revoked }
            onRevoked(client)
        }
        startBackground(client, sync)
        device.paired = true
        return Phase.Paired(credentials, client, sync)
    }

    /**
     * 常駐と通知の判断を始める: つなぐのはアプリが前面か常駐の間 (切れるのは前面でなくなり常駐も終わったとき)、
     * 常駐 ([keepAlive]) は [BackgroundController] が決める。
     */
    private fun startBackground(client: YhtyeClient, sync: AppStore) {
        stopBackground()
        val job = SupervisorJob(scope.coroutineContext[Job])
        background = job
        val s = CoroutineScope(scope.coroutineContext + job)
        val controller = BackgroundController(
            scope = s,
            mode = device.mode,
            connection = client.state,
            app = sync.state,
            foreground = foreground,
            usersTurns = sync.usersTurns,
            notifier = notifications,
            graceMs = keepAliveGraceMs,
        )
        s.launch { controller.keepAlive.collect { _keepAlive.value = it } }
        s.launch {
            var wanted = false
            controller.wantConnected.collect { want ->
                if (want) {
                    client.start()
                } else if (wanted && !client.state.value.isFinal()) {
                    // 拒否・失効の状態は残す (消すと「待機」に見えて、また常駐の理由になり、つなぎ直しを繰り返してしまう)
                    client.stop()
                }
                wanted = want
            }
        }
    }

    private fun ConnectionState.isFinal(): Boolean = this is ConnectionState.Revoked || this is ConnectionState.Rejected

    private fun stopBackground() {
        background?.cancel()
        background = null
    }

    private suspend fun onRevoked(client: YhtyeClient) {
        val current = _phase.value as? Phase.Paired ?: return
        if (current.client !== client) return
        stopBackground()
        current.sync.close()
        client.close()
        clientPrefs.clear()
        store.clear()
        notifications.cancelAll()
        // 前面なら案内の画面がすぐ見えるので通知は要らない
        if (!foreground.value) notifications.revoked()
        _phase.value = unpairedStart().copy(revoked = true)
    }
}
