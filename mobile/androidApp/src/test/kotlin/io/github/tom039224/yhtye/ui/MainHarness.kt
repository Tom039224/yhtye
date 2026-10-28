package io.github.tom039224.yhtye.ui

import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import io.github.tom039224.yhtye.YhtyeTheme
import io.github.tom039224.yhtye.background.OpenChatRequest
import io.github.tom039224.yhtye.shared.api.generated.Snapshot
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.LoadPhase
import io.github.tom039224.yhtye.ui.panels.MainView
import io.github.tom039224.yhtye.ui.settings.DevicePane
import io.github.tom039224.yhtye.ui.settings.UnavailableDevicePane
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

    /** 設定の「この端末」(テストが差し替える)。 */
    var device: DevicePane = UnavailableDevicePane

    /** 通知のタップで頼まれたチャット (テストが入れる)。画面が開き終えたら [handled] に積む。 */
    val openChat = mutableStateOf<OpenChatRequest?>(null)
    val handled = mutableListOf<OpenChatRequest>()

    /**
     * [snapshot] のプロジェクトを開いて画面を出す。[configure] はコアの答え (プロジェクトの一覧・git) を変える。[view] は最初の
     * ビュー (アプリはダッシュボードから始まるが、多くのテストは会話を見るのでコーディングから)。[withProject] が false なら
     * プロジェクトを開かない (一覧だけ読む)。
     */
    fun open(
        snapshot: Snapshot = recording.end,
        insets: WindowInsets = WindowInsets(0),
        view: MainView = MainView.Work,
        withProject: Boolean = true,
        configure: TestCore.() -> Unit = {},
    ) {
        core = TestCore(snapshot, recording.events.filter { !it.live && it.seq <= snapshot.seq }).apply(configure)
        val s = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        scope = s
        sync = AppStore(core, s)
        sync.setForeground(false)
        sync.start()
        rule.setContent {
            YhtyeTheme(darkTheme = true) {
                MainScreen(
                    "device",
                    sync,
                    onReconnect = {},
                    onUnpair = { unpaired++ },
                    gestureInsets = insets,
                    device = device,
                    openChat = openChat.value,
                    onOpenChatHandled = {
                        handled += it
                        openChat.value = null
                    },
                    initialView = view,
                )
            }
        }
        s.launch { sync.refreshProjects() }
        if (withProject) {
            s.launch { sync.openProject(PROJECT.path) }
            settle { sync.state.value.project?.phase == LoadPhase.Ready && sync.state.value.git?.overview != null }
        } else {
            settle { sync.state.value.projects.isNotEmpty() || core.projects.isEmpty() }
        }
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

    /** 何も起きないことを確かめるために、アニメーションや遅れて起きる操作 (メンションの確定は 300ms 後) が終わるくらい (約 1 秒) 進める。 */
    fun settleFor() {
        var rounds = 0
        settle { ++rounds >= QUIET_ROUNDS }
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

    /** 画面全体を、タッチスロップを越えてから幅の [pulled] (割合) だけ横へ動かして離す (速さは [dpPerSecond])。 */
    fun dragScreen(pulled: Float, leftward: Boolean = false, dpPerSecond: Float = SLOW_DP_PER_SECOND) {
        rule.onRoot().performTouchInput {
            val from = Offset(width * (if (leftward) 0.9f else 0.1f), height / 2f)
            dragFrom(from, width * pulled, leftward, dpPerSecond)
        }
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
        const val QUIET_ROUNDS = 60
    }
}

private fun AndroidComposeTestRule<*, *>.onAllNodesWithTagCount(tag: String): Int =
    onAllNodes(androidx.compose.ui.test.hasTestTag(tag)).fetchSemanticsNodes().size
