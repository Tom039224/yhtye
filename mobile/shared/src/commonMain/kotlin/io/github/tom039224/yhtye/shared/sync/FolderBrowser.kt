package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.DirectoryEntry
import io.github.tom039224.yhtye.shared.client.CoreConnection
import io.github.tom039224.yhtye.shared.client.RpcException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext
import kotlin.coroutines.ContinuationInterceptor

/**
 * コアが動いているマシン (ホスト) のフォルダの閲覧 ([FolderBrowser.state])。端末のファイルではなくホストのもの
 * (プロジェクトはホストのフォルダ)。フォルダだけが出る (ファイルは出ない)。
 */
data class FolderBrowserState(
    /** 開いているフォルダの絶対パス (シンボリックリンクは解決済み)。まだ何も開いていなければ null。 */
    val path: String? = null,
    /** その親 (ルートでは null)。 */
    val parent: String? = null,
    /** コアのユーザーのホームディレクトリ。 */
    val home: String? = null,
    /** 開いているフォルダ自身が git リポジトリ (プロジェクトとして開く候補)。 */
    val isGitRepo: Boolean = false,
    /** その直下のフォルダ (名前順、大文字小文字は区別しない)。 */
    val entries: List<DirectoryEntry> = emptyList(),
    /** 先頭が `.` の隠しフォルダも出す。 */
    val showHidden: Boolean = false,
    val loading: Boolean = false,
    /** 最後に開こうとして失敗した理由 (別のフォルダを開き始めると消える。開いていたフォルダはそのまま残る)。 */
    val error: String? = null,
) {
    /** 1 つ上のフォルダへ行ける。 */
    val canGoUp: Boolean get() = parent != null
}

/**
 * ホストのフォルダを辿る状態 (`list_directory`)。[open] / [up] / [home] で開き、[openAsProject] で選んだフォルダを
 * プロジェクトとして開く。続けて呼ぶと最後の呼びだけが状態を変える (前の答えは捨てる)。失敗しても開いていたフォルダは
 * そのまま ([FolderBrowserState.error] だけが付く): 読めないフォルダに触れても今の場所が消えない。
 *
 * 状態の変更は [scope] のディスパッチャを並列度 1 に絞って行うので、どのスレッドから呼んでもよい。
 *
 * @param openProject プロジェクトを開く操作 ([AppStore.openProject])。
 */
class FolderBrowser(
    private val core: CoreConnection,
    scope: CoroutineScope,
    private val openProject: suspend (path: String) -> Unit,
) {
    private val confined: CoroutineDispatcher =
        (scope.coroutineContext[ContinuationInterceptor] as? CoroutineDispatcher ?: Dispatchers.Default).limitedParallelism(1)

    private val _state = MutableStateFlow(FolderBrowserState())

    val state: StateFlow<FolderBrowserState> = _state.asStateFlow()

    // [confined] でだけ触る
    private var token = 0L

    /**
     * [path] (ホストの絶対パス。`~` や `~/…` はホームから) のフォルダを開く。null か空はホームディレクトリ。
     * 隠しフォルダを出すかは今の [FolderBrowserState.showHidden]。
     */
    suspend fun open(path: String? = null) {
        withContext(confined) { load(path, _state.value.showHidden) }
    }

    /** 1 つ上のフォルダを開く (ルートでは何もしない)。 */
    suspend fun up() {
        withContext(confined) {
            val parent = _state.value.parent ?: return@withContext
            load(parent, _state.value.showHidden)
        }
    }

    /** コアのユーザーのホームディレクトリを開く。 */
    suspend fun home() {
        open(null)
    }

    /** 開いているフォルダを読み直す (まだ何も開いていなければホームを開く)。 */
    suspend fun reload() {
        open(_state.value.path)
    }

    /** 隠しフォルダを出す・出さないを切り替えて、開いているフォルダがあれば読み直す。 */
    suspend fun setShowHidden(value: Boolean) {
        withContext(confined) {
            if (_state.value.showHidden == value) return@withContext
            _state.value = _state.value.copy(showHidden = value)
            // 何も開いていなければ切り替えを覚えるだけ (次に開くときに使う)
            val path = _state.value.path ?: return@withContext
            load(path, value)
        }
    }

    /**
     * フォルダ [path] (既定は今開いているフォルダ) をプロジェクトとして開く ([AppStore.openProject]: 失敗は
     * [AppState.errors] に出る)。何も開いていないときに [path] を省くと何もしない。
     */
    suspend fun openAsProject(path: String? = _state.value.path) {
        openProject(path ?: return)
    }

    private suspend fun load(path: String?, showHidden: Boolean) {
        val mine = ++token
        _state.value = _state.value.copy(loading = true, error = null)
        try {
            val listing = core.expect<ApiResponse.Directory>(ApiCommand.ListDirectory(path, showHidden.takeIf { it })).directory
            if (mine != token) return
            _state.value = _state.value.copy(
                path = listing.path,
                parent = listing.parent,
                home = listing.home,
                isGitRepo = listing.isGitRepo,
                entries = listing.entries,
                loading = false,
            )
        } catch (e: RpcException) {
            if (mine == token) _state.value = _state.value.copy(loading = false, error = describe(e))
        } catch (e: CancellationException) {
            if (mine == token) _state.value = _state.value.copy(loading = false)
            throw e
        }
    }
}
