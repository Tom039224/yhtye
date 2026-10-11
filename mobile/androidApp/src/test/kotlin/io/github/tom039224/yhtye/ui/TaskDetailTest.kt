package io.github.tom039224.yhtye.ui

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.compose.ui.unit.dp
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * タスク詳細 (積み重ねの全画面): カードのタップで開き、セッションのタブ・Step の結果・エージェントの出力を出す。
 * 左→右のスワイプ (足りなければ戻る) と戻るで閉じ (詳細 → パネル → 中央)、メニューは `@` と中止だけ。
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w400dp-h800dp")
class TaskDetailTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val h = MainHarness(rule)

    @After
    fun tearDown() = h.close()

    private fun openDetail(task: String) {
        h.openRight()
        rule.onNodeWithTag("task-$task").performClick()
        h.settle { h.exists("task-detail") }
    }

    private fun back() {
        rule.runOnUiThread { rule.activity.onBackPressedDispatcher.onBackPressed() }
        rule.waitForIdle()
    }

    @Test
    fun `a tapped card opens its detail with the sessions, step results and output`() {
        h.open()
        openDetail("T-1")
        rule.onNodeWithTag("detail-title").assertTextEquals("add hello.txt")
        rule.onNodeWithText("T-1 · G-1").assertExists()
        assertEquals(3, rule.onAllNodes(hasTestTag("step-result"), useUnmergedTree = true).fetchSemanticsNodes().size)
        // 最後のセッション (review-1) から
        val review = rule.onNodeWithTag("session-tab-T-1/review-1").fetchSemanticsNode().config[SemanticsProperties.Selected]
        assertTrue(review)
        rule.onNode(hasText("Looks good.", substring = true)).assertExists()
        rule.onNodeWithTag("session-tab-T-1/implementer").performClick()
        rule.waitForIdle()
        rule.onNode(hasText("Writing the file.", substring = true)).assertExists()
        // 終わったタスクのメニューに中止は無い
        rule.onNodeWithTag("detail-menu").performClick()
        rule.onNodeWithTag("detail-mention").assertExists()
        assertFalse(h.exists("detail-cancel"))
    }

    @Test
    fun `the help of a task is shown as a state only`() {
        h.open(activeGroup(h))
        openDetail("T-2")
        // 下の右パネルのカードにも出ているので、詳細の中のものを見る
        val inDetail = hasAnyAncestor(hasTestTag("task-detail"))
        rule.onNode(hasText("注文スキーマが v2 と v3 で混在しています") and inDetail, useUnmergedTree = true).assertExists()
        rule.onNodeWithText("T-2 のエージェントは未起動です", useUnmergedTree = true).assertExists()
        assertEquals(0, rule.onAllNodes(hasText("回答", substring = true)).fetchSemanticsNodes().size)
    }

    @Test
    fun `a left to right swipe closes the detail and a short slow one does not`() {
        h.open()
        openDetail("T-1")
        h.swipeScreen(0.2f, 0.3f, durationMillis = 1_000)
        h.settle { true }
        assertTrue(h.exists("task-detail"))
        // 右→左は何もしない (パネルも動かない)
        h.swipeScreen(0.9f, 0.1f)
        h.settle { true }
        assertTrue(h.exists("task-detail"))

        h.swipeScreen(0.2f, 0.9f)
        h.settle { !h.exists("task-detail") }
        // 下の右パネルは開いたまま
        assertTrue(h.exists("right-panel"))
    }

    @Test
    fun `a swipe from the screen edge is left to the system back gesture`() {
        h.open(insets = WindowInsets(left = 24.dp, right = 24.dp))
        openDetail("T-1")
        rule.onRoot().performTouchInput { swipe(Offset(5f, centerY), Offset(width * 0.9f, centerY), 300) }
        rule.waitForIdle()
        h.settle { true }
        assertTrue(h.exists("task-detail"))
    }

    @Test
    fun `back closes the detail first, then the panel`() {
        h.open()
        openDetail("T-1")
        back()
        h.settle { !h.exists("task-detail") }
        assertTrue(h.exists("right-panel"))
        back()
        h.settle { !h.exists("right-panel") }
    }

    @Test
    fun `the header back button closes the detail`() {
        h.open()
        openDetail("T-1")
        rule.onNodeWithTag("detail-back").performClick()
        h.settle { !h.exists("task-detail") }
    }

    @Test
    fun `the menu quotes the task and returns to the center`() {
        h.open()
        openDetail("T-1")
        rule.onNodeWithTag("detail-menu").performClick()
        rule.onNodeWithTag("detail-mention").performClick()
        h.settle { h.exists("mention-T-1") && !h.exists("task-detail") }
        assertFalse(h.exists("right-panel"))
    }

    @Test
    fun `the menu stops a running task after asking`() {
        h.open(activeGroup(h))
        openDetail("T-1")
        rule.onNodeWithTag("detail-menu").performClick()
        rule.onNodeWithTag("detail-cancel").performClick()
        rule.onNodeWithTag("cancel-confirm").performClick()
        h.settle { h.core.callsOf<ApiCommand.CancelTask>().isNotEmpty() }
        assertEquals("T-1", h.core.callsOf<ApiCommand.CancelTask>().single().task)
    }
}
