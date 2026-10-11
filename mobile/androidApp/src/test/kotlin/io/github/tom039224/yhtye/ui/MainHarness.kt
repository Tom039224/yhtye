package io.github.tom039224.yhtye.ui

import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import io.github.tom039224.yhtye.YhtyeTheme
import io.github.tom039224.yhtye.shared.api.generated.Snapshot
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.LoadPhase
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.robolectric.Shadows.shadowOf

/**
 * ホーム ([MainScreen]) を本物の同期層 ([AppStore]) と偽のコア ([TestCore]) で動かす (パネル・タスク詳細のテスト用)。
 * 同期層のスコープはテストの規則が Main を差し替えた後に作る。
 */
class MainHarness(private val rule: AndroidComposeTestRule<*, ComponentActivity>) {
    val recording = readRecording()
    lateinit var core: TestCore
    lateinit var sync: AppStore
    private var scope: CoroutineScope? = null
    var unpaired = 0

    /** [snapshot] のプロジェクトを開いて画面を出す。[configure] はコアの答え (プロジェクトの一覧・git) を変える。 */
    fun open(snapshot: Snapshot = recording.end, insets: WindowInsets = WindowInsets(0), configure: TestCore.() -> Unit = {}) {
        core = TestCore(snapshot, recording.events.filter { !it.live && it.seq <= snapshot.seq }).apply(configure)
        val s = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        scope = s
        sync = AppStore(core, s)
        sync.setForeground(false)
        sync.start()
        rule.setContent {
            YhtyeTheme(darkTheme = true) {
                MainScreen("device", sync, onReconnect = {}, onUnpair = { unpaired++ }, gestureInsets = insets)
            }
        }
        s.launch { sync.refreshProjects() }
        s.launch { sync.openProject(PROJECT.path) }
        settle { sync.state.value.project?.phase == LoadPhase.Ready && sync.state.value.git?.overview != null }
        rule.waitForIdle()
    }

    fun close() {
        if (::sync.isInitialized) sync.close()
        scope?.cancel()
    }

    /**
     * [done] になるまで main looper (同期層のコルーチン) と画面を進める。Compose の `waitUntil` は Robolectric の
     * looper に積まれた (Dispatchers.Main の) コルーチンを流さないことがあるので、自分で回す。
     */
    fun settle(done: () -> Boolean) {
        repeat(SETTLE_ROUNDS) {
            shadowOf(Looper.getMainLooper()).idle()
            rule.mainClock.advanceTimeBy(FRAME_MS)
            rule.waitForIdle()
            if (done()) return
        }
        error("condition not reached")
    }

    /** 中身が出ているか (パネルは開いているときだけ合成される)。 */
    fun exists(tag: String): Boolean = rule.onAllNodesWithTagCount(tag) > 0

    fun openLeft() {
        rule.onNodeWithTag("open-left").performClick()
        rule.waitForIdle()
    }

    fun openRight() {
        rule.onNodeWithTag("open-right").performClick()
        rule.waitForIdle()
    }

    /** 画面全体を横にスワイプする ([fromX] / [toX] は幅に対する割合)。 */
    fun swipeScreen(fromX: Float, toX: Float, durationMillis: Long = 300) {
        rule.onRoot().performTouchInput {
            swipe(Offset(width * fromX, height / 2f), Offset(width * toX, height / 2f), durationMillis)
        }
        rule.waitForIdle()
    }

    private companion object {
        const val SETTLE_ROUNDS = 200
        const val FRAME_MS = 16L
    }
}

private fun AndroidComposeTestRule<*, *>.onAllNodesWithTagCount(tag: String): Int =
    onAllNodes(androidx.compose.ui.test.hasTestTag(tag)).fetchSemanticsNodes().size
