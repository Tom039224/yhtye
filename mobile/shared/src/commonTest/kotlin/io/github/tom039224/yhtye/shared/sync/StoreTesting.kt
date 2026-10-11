package io.github.tom039224.yhtye.shared.sync

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runCurrent
import kotlin.test.fail

/** テストの [AppStore]。使用量と ping は既定で止める ([foreground] = false。前面の間だけ読むため)。 */
@OptIn(ExperimentalCoroutinesApi::class)
fun TestScope.newStore(
    core: FakeCore,
    prefs: ClientPrefs = MemoryPrefs(),
    options: AppStore.Options = AppStore.Options(),
    foreground: Boolean = false,
): AppStore {
    val store = AppStore(core, backgroundScope, prefs, options = options, now = { currentTime })
    store.setForeground(foreground)
    store.start()
    runCurrent()
    return store
}

/** 表示中のプロジェクト (無ければ失敗)。 */
fun AppStore.view(): ProjectView = state.value.project ?: fail("no project")

/** プロジェクトを開き、読み込みと続きの要求 (git・open_chat) が終わるまで進める。 */
@OptIn(ExperimentalCoroutinesApi::class)
suspend fun TestScope.openAndSettle(store: AppStore, path: String = PROJECT.path) {
    store.openProject(path)
    runCurrent()
    if (store.view().phase != LoadPhase.Ready) fail("not ready: ${store.view().phase}")
}

/** イベントを押し込み、ストアが処理し終えるまで進める。 */
@OptIn(ExperimentalCoroutinesApi::class)
fun TestScope.push(core: FakeCore, vararg events: io.github.tom039224.yhtye.shared.api.generated.ApiEvent) {
    events.forEach(core::emit)
    runCurrent()
}

/** [flow] に来たものを集める (すぐ購読する)。 */
@OptIn(ExperimentalCoroutinesApi::class)
fun <T> TestScope.collected(flow: Flow<T>): List<T> {
    val seen = mutableListOf<T>()
    backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { flow.collect { seen += it } }
    return seen
}
