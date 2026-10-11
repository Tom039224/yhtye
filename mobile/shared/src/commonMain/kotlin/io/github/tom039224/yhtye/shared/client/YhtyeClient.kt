package io.github.tom039224.yhtye.shared.client

import io.github.tom039224.yhtye.shared.api.ApiJson
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.WsReply
import io.github.tom039224.yhtye.shared.api.generated.WsRequest
import io.github.tom039224.yhtye.shared.api.generated.WsServerMessage
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull

/**
 * PC の Yhtye と WebSocket で話すクライアント (`core-design.md` §10 / §18)。
 *
 * - [call]: 要求 `{id, cmd}` を送り、同じ `id` の応答を待つ。応答は順不同で届く (PC は要求を並行に処理する)。
 * - [events]: PC が流すイベント。取りこぼしがあり得る (切断中・バッファあふれ)。利用側は `seq` の飛びや
 *   [ConnectionState.Connected.session] の変化で状態を取り直す。
 * - [state]: 接続状態。切れたら [Config.backoff] で待って再接続する。401 では再接続をやめ [ConnectionState.Revoked]、
 *   403 では [ConnectionState.Rejected] で止まる。
 *
 * [start] / [stop] / [close] の指示は順に処理する (どのスレッドから呼んでもよい)。
 */
class YhtyeClient(
    private val transport: WsTransport,
    private val scope: CoroutineScope,
    private val config: Config = Config(),
) {
    data class Config(
        /** [call] の既定の待ち時間。プロジェクトを開くなど時間の掛かる要求は個別に延ばす。 */
        val callTimeoutMs: Long = 30_000,
        /** 1 回の接続の試みに掛ける時間の上限。 */
        val connectTimeoutMs: Long = 20_000,
        val backoff: Backoff = Backoff(),
        /** 受け取ったイベントを溜めておく数。あふれたら古いものから捨てる。 */
        val eventBufferSize: Int = 1024,
    )

    private val _state = MutableStateFlow<ConnectionState>(ConnectionState.Idle)
    val state: StateFlow<ConnectionState> = _state.asStateFlow()

    private val _events = MutableSharedFlow<ApiEvent>(
        extraBufferCapacity = config.eventBufferSize,
        onBufferOverflow = BufferOverflow.DROP_OLDEST,
    )
    val events: SharedFlow<ApiEvent> = _events.asSharedFlow()

    /** [connection] と [pending] と [_state] の更新を守る。[_state] が Connected の間だけ [connection] がある。 */
    private val lock = Mutex()
    private var connection: WsConnection? = null
    private val pending = HashMap<Long, CompletableDeferred<ApiResponse>>()
    private var nextId = 1L
    private var session = 0L

    private val control = Channel<Control>(Channel.UNLIMITED)
    private val wakeUp = Channel<Unit>(Channel.CONFLATED)

    /** 接続を保つループ。[control] を処理するコルーチンだけが触る。 */
    private var loop: Job? = null

    private enum class Control { Start, Stop }

    init {
        scope.launch {
            for (command in control) {
                when (command) {
                    Control.Start -> onStart()
                    Control.Stop -> onStop()
                }
            }
            onStop()
        }
    }

    /**
     * つなぐ (つながるまで再接続し続ける)。再接続を待っている間なら待たずにすぐ試す
     * (アプリが前面に戻ったとき・「再試行」)。[ConnectionState.Revoked] の後は何もしない。
     */
    fun start() {
        control.trySend(Control.Start)
    }

    /** 切断して [ConnectionState.Idle] にする (応答待ちの要求は [ConnectionLostException])。 */
    fun stop() {
        control.trySend(Control.Stop)
    }

    /** 切断し、このクライアントを捨てる (以後 [start] は効かない)。 */
    fun close() {
        control.trySend(Control.Stop)
        control.close()
    }

    /**
     * [command] を実行して応答を返す。
     *
     * つながっていなければ、つながるまで待つ ([timeoutMs] の内で)。
     *
     * @throws ApiException PC がエラーで答えた。
     * @throws RpcTimeoutException 送ったが時間内に応答が無かった。
     * @throws ConnectionLostException 応答の前に切れた (再送はしない。PC では実行されたかもしれない)。
     * @throws NotConnectedException 時間内につながらなかった、または失効・拒否されている。
     */
    suspend fun call(command: ApiCommand, timeoutMs: Long = config.callTimeoutMs): ApiResponse {
        val reply = CompletableDeferred<ApiResponse>()
        var requestId: Long? = null
        try {
            val response = withTimeoutOrNull(timeoutMs) {
                val (id, conn) = register(reply)
                requestId = id
                val text = ApiJson.encodeToString(WsRequest.serializer(), WsRequest(id, command))
                try {
                    conn.send(text)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    throw ConnectionLostException()
                }
                reply.await()
            }
            return response ?: when (requestId) {
                null -> throw NotConnectedException(_state.value)
                else -> throw RpcTimeoutException(command, timeoutMs)
            }
        } finally {
            requestId?.let { id -> withContext(NonCancellable) { lock.withLock { pending.remove(id) } } }
        }
    }

    /** つながるのを待ち、応答の受け皿を登録する。 */
    private suspend fun register(reply: CompletableDeferred<ApiResponse>): Pair<Long, WsConnection> {
        while (true) {
            val seen = _state.first {
                it is ConnectionState.Connected || it is ConnectionState.Revoked || it is ConnectionState.Rejected
            }
            if (seen !is ConnectionState.Connected) throw NotConnectedException(seen)
            lock.withLock {
                val conn = connection
                if (conn != null) {
                    val id = nextId++
                    pending[id] = reply
                    return id to conn
                }
            }
            // 切れた直後 (状態はまだ Connected): 次の状態を待つ
            _state.first { it != seen }
        }
    }

    private fun onStart() {
        if (_state.value == ConnectionState.Revoked) return
        if (loop?.isActive == true) {
            wakeUp.trySend(Unit)
            return
        }
        loop = scope.launch { run() }
    }

    private suspend fun onStop() {
        loop?.let {
            it.cancel()
            it.join()
        }
        loop = null
        withContext(NonCancellable) {
            lock.withLock {
                connection = null
                failPending()
                if (_state.value != ConnectionState.Revoked) _state.value = ConnectionState.Idle
            }
        }
    }

    /** 接続し、切れたら待って再接続する。拒否 (401 / 403) で終わる。 */
    private suspend fun run() {
        var failures = 0
        while (true) {
            setState(ConnectionState.Connecting(failures))
            val conn = try {
                withTimeoutOrNull(config.connectTimeoutMs) { transport.connect() }
            } catch (e: CancellationException) {
                throw e
            } catch (e: WsRefusedException) {
                setState(
                    when (e.refusal) {
                        Refusal.Unauthorized -> ConnectionState.Revoked
                        Refusal.Forbidden -> ConnectionState.Rejected(e.message)
                    },
                )
                return
            } catch (e: Exception) {
                failures = waitBeforeRetry(failures + 1, e.message ?: e::class.simpleName)
                continue
            }
            if (conn == null) {
                failures = waitBeforeRetry(failures + 1, "timed out")
                continue
            }
            val reason = serve(conn)
            failures = waitBeforeRetry(1, reason)
        }
    }

    /** 再接続まで待つ。[start] で起こされたら失敗の数を 0 に戻す。戻り値は次の試みの失敗の数。 */
    private suspend fun waitBeforeRetry(failures: Int, reason: String?): Int {
        val delayMs = config.backoff.delayMs(failures)
        setState(ConnectionState.WaitingToReconnect(failures, delayMs, reason))
        val woken = withTimeoutOrNull(delayMs) { wakeUp.receive() } != null
        return if (woken) 0 else failures
    }

    /** 接続が切れるまでメッセージを読む。戻り値は切れた理由。 */
    private suspend fun serve(conn: WsConnection): String {
        wakeUp.tryReceive() // つながる前の「すぐ試して」は済んだ
        lock.withLock {
            session += 1
            connection = conn
            _state.value = ConnectionState.Connected(session)
        }
        try {
            while (true) {
                val text = conn.receive() ?: return "closed by the PC"
                handle(text)
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            return e.message ?: "connection error"
        } finally {
            withContext(NonCancellable) {
                lock.withLock {
                    connection = null
                    failPending()
                }
                wakeUp.tryReceive()
                runCatching { conn.close() }
            }
        }
    }

    private suspend fun handle(text: String) {
        val message = try {
            ApiJson.decodeFromString(WsServerMessage.Serializer, text)
        } catch (e: IllegalArgumentException) { // SerializationException を含む。壊れたメッセージは捨てる
            return
        }
        when (message) {
            is WsServerMessage.Reply -> when (val reply = message.value) {
                is WsReply.Ok -> resolve(reply.id) { it.complete(reply.ok) }
                is WsReply.Err -> resolve(reply.id) { it.completeExceptionally(ApiException(reply.err)) }
                // 知らない形の応答: 対応する要求はタイムアウトになる
                is WsReply.Unrecognized -> Unit
            }
            is WsServerMessage.Event -> _events.tryEmit(message.event)
            is WsServerMessage.Unrecognized -> Unit
        }
    }

    private suspend fun resolve(id: Long, complete: (CompletableDeferred<ApiResponse>) -> Unit) {
        val reply = lock.withLock { pending.remove(id) } ?: return // 待つのをやめた要求
        complete(reply)
    }

    private suspend fun setState(next: ConnectionState) {
        lock.withLock { _state.value = next }
    }

    /** [lock] を持って呼ぶ。 */
    private fun failPending() {
        pending.values.forEach { it.completeExceptionally(ConnectionLostException()) }
        pending.clear()
    }
}
