package io.github.tom039224.yhtye.session

import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.KtorWsTransport
import io.github.tom039224.yhtye.shared.client.YhtyeClient
import io.github.tom039224.yhtye.shared.credentials.CredentialStore
import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.github.tom039224.yhtye.shared.pairing.InvalidPairingInput
import io.github.tom039224.yhtye.shared.pairing.InvalidPairingInputException
import io.github.tom039224.yhtye.shared.pairing.PairingClient
import io.github.tom039224.yhtye.shared.pairing.PairingResult
import io.github.tom039224.yhtye.shared.pairing.PairingTarget
import io.github.tom039224.yhtye.shared.pairing.parsePairingUri
import io.ktor.client.HttpClient
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/**
 * アプリ全体の状態: ペアリングしているか、PC への接続 ([YhtyeClient])、受け取ったディープリンク。
 *
 * @param scope メインスレッドのスコープ (状態の変更はここで順に行う)。
 * @param clientScope 通信 (受信した JSON の解析) を動かすスコープ。
 */
class Session(
    private val store: CredentialStore,
    private val http: HttpClient,
    private val scope: CoroutineScope,
    private val clientScope: CoroutineScope,
) {
    sealed interface Phase {
        data object Loading : Phase

        /** @property revoked PC で失効されたので戻ってきた (案内を出す)。 */
        data class Unpaired(val revoked: Boolean = false) : Phase

        data class Paired(val credentials: Credentials, val client: YhtyeClient) : Phase
    }

    /** 外から届いたペアリングの指示 (ディープリンク)。画面が受け取ったら [consumeIncoming] する。 */
    sealed interface Incoming {
        data class Target(val target: PairingTarget) : Incoming

        data class Invalid(val reason: InvalidPairingInput) : Incoming
    }

    private val pairing = PairingClient(http)
    private var foreground = false

    /** 今の [YhtyeClient] の失効を待つコルーチン。 */
    private var revocationWatch: Job? = null

    private val _phase = MutableStateFlow<Phase>(Phase.Loading)
    val phase: StateFlow<Phase> = _phase.asStateFlow()

    private val _incoming = MutableStateFlow<Incoming?>(null)
    val incoming: StateFlow<Incoming?> = _incoming.asStateFlow()

    init {
        scope.launch {
            val stored = store.load()
            _phase.value = if (stored != null) paired(stored) else Phase.Unpaired()
        }
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
        current.client.close()
        _phase.value = Phase.Unpaired()
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
        foreground = true
        (_phase.value as? Phase.Paired)?.client?.start()
    }

    /** アプリが裏に回った: 切る (前面に戻ったらつなぎ直して状態を取り直す)。 */
    fun onBackground() {
        foreground = false
        (_phase.value as? Phase.Paired)?.client?.stop()
    }

    private fun paired(credentials: Credentials): Phase.Paired {
        val client = YhtyeClient(KtorWsTransport(http, credentials), clientScope)
        revocationWatch?.cancel()
        revocationWatch = scope.launch {
            client.state.first { it == ConnectionState.Revoked }
            onRevoked(client)
        }
        if (foreground) client.start()
        return Phase.Paired(credentials, client)
    }

    private suspend fun onRevoked(client: YhtyeClient) {
        val current = _phase.value as? Phase.Paired ?: return
        if (current.client !== client) return
        client.close()
        store.clear()
        _phase.value = Phase.Unpaired(revoked = true)
    }
}
