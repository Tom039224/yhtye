package io.github.tom039224.yhtye.shared.update

import io.github.tom039224.yhtye.shared.version.AppVersion
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlin.time.Clock
import kotlin.time.ExperimentalTime

/** 自動の確認の間隔: 最後の確認からこれだけたっていれば、アプリが前面に来たときに確認する。 */
const val UPDATE_CHECK_INTERVAL_MS: Long = 24 * 60 * 60 * 1_000L

/** 確認に失敗したあと、自動でやり直すまでの最短の間 (前面に来るたびに API を叩かない)。「今すぐ確認」はこれに縛られない。 */
const val UPDATE_RETRY_AFTER_FAILURE_MS: Long = 60 * 60 * 1_000L

/** 更新の確認の状態 (設定画面と、左パネルの印)。 */
data class UpdateState(
    /** 自動の確認をする (設定の「更新を確認する」)。切ると GitHub には何も送らない。 */
    val enabled: Boolean = true,
    val checking: Boolean = false,
    /** 最後に確認できた時刻 (エポックからのミリ秒)。失敗は含めない。 */
    val checkedAt: Long? = null,
    /** 自分より新しい版 (無ければ null)。 */
    val available: AvailableUpdate? = null,
    /** 最後の確認が失敗した理由 (設定画面にだけ出す)。成功すると消える。 */
    val failure: String? = null,
)

/** 端末に残す確認の記録 (アプリを閉じても、24 時間おきの間隔と印を覚えておく)。 */
data class StoredUpdate(
    val enabled: Boolean = true,
    val checkedAt: Long? = null,
    /** 最後の確認で見つけた新しい版のタグ。 */
    val availableTag: String? = null,
)

/** [StoredUpdate] の置き場。ペアリングを解除しても残す (PC のものではなく、この端末のアプリのもの)。 */
interface UpdateStore {
    fun load(): StoredUpdate

    fun save(stored: StoredUpdate)
}

/** メモリだけ (テスト)。 */
class MemoryUpdateStore(var stored: StoredUpdate = StoredUpdate()) : UpdateStore {
    override fun load(): StoredUpdate = stored

    override fun save(stored: StoredUpdate) {
        this.stored = stored
    }
}

/**
 * アプリ内の更新の確認。GitHub Releases の `android-v*` のリリースを読み、[current] より新しい版があれば知らせる
 * (印と設定画面。通知は出さない。入れるのはブラウザ経由で、アプリはダウンロードもインストールもしない)。
 *
 * - 自動の確認: アプリが前面に来たとき ([onForeground])、[enabled] で、最後の確認から [UPDATE_CHECK_INTERVAL_MS] 以上
 *   たっていれば。失敗した後は [UPDATE_RETRY_AFTER_FAILURE_MS] 待つ。
 * - 手動の確認 ([checkNow]): 設定の「今すぐ確認」。間隔には縛られない。切っている間は何もしない。
 * - 失敗 (オフライン・回数制限など) は黙って捨てる。[UpdateState.failure] に理由を残し、設定画面にだけ出す。
 *
 * @param scope 状態を変えるスコープ (メインスレッド)。
 */
class UpdateChecker(
    private val source: ReleaseSource,
    private val current: AppVersion,
    private val store: UpdateStore,
    private val scope: CoroutineScope,
    private val now: () -> Long = ::epochMillis,
) {
    private val _state = MutableStateFlow(initial())
    val state: StateFlow<UpdateState> = _state.asStateFlow()

    private var job: Job? = null

    /** 最後に失敗した時刻 (このプロセスの中だけ。やり直しの間隔に使う)。 */
    private var failedAt: Long? = null

    private fun initial(): UpdateState {
        val stored = store.load()
        // 前に見つけた版は、もう入れた (自分がそれ以上) なら印を出さない
        val available = if (stored.enabled) stored.availableTag?.let(::androidUpdateOfTag)?.takeIf { it.version > current } else null
        return UpdateState(enabled = stored.enabled, checkedAt = stored.checkedAt, available = available)
    }

    /** アプリが前面に来た。確認する時期なら確認を始める。 */
    fun onForeground() {
        if (isDue()) launchCheck()
    }

    /** 「今すぐ確認」。確認中・切っているときは何もしない。 */
    fun checkNow() {
        if (_state.value.enabled) launchCheck()
    }

    /** 「更新を確認する」の切り替え。切ると進行中の確認をやめて印を消し、入れると確認する。 */
    fun setEnabled(enabled: Boolean) {
        if (_state.value.enabled == enabled) return
        if (enabled) {
            _state.update { it.copy(enabled = true) }
            persist()
            launchCheck()
        } else {
            job?.cancel()
            job = null
            _state.update { it.copy(enabled = false, checking = false, available = null, failure = null) }
            persist()
        }
    }

    private fun isDue(): Boolean {
        val s = _state.value
        if (!s.enabled || s.checking) return false
        val t = now()
        val sinceFailure = failedAt?.let { t - it }
        if (sinceFailure != null && sinceFailure in 0 until UPDATE_RETRY_AFTER_FAILURE_MS) return false
        val sinceCheck = s.checkedAt?.let { t - it } ?: return true
        // 時計が戻されていたら、待たずに確認する
        return sinceCheck < 0 || sinceCheck >= UPDATE_CHECK_INTERVAL_MS
    }

    private fun launchCheck() {
        if (job?.isActive == true) return
        _state.update { it.copy(checking = true) }
        job = scope.launch { check() }
    }

    private suspend fun check() {
        val releases = try {
            source.fetch()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            failedAt = now()
            _state.update { it.copy(checking = false, failure = e.message ?: e::class.simpleName) }
            return
        }
        val available = newestAndroidUpdate(releases, current)
        _state.update { it.copy(checking = false, checkedAt = now(), available = available, failure = null) }
        failedAt = null
        persist()
    }

    private fun persist() {
        val s = _state.value
        store.save(StoredUpdate(s.enabled, s.checkedAt, s.available?.tag))
    }
}

@OptIn(ExperimentalTime::class)
private fun epochMillis(): Long = Clock.System.now().toEpochMilliseconds()
