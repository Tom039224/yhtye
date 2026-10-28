package io.github.tom039224.yhtye.ui

import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.github.tom039224.yhtye.YhtyeTheme
import io.github.tom039224.yhtye.ui.conversation.Markdown
import io.github.tom039224.yhtye.ui.shell.PEEK
import io.github.tom039224.yhtye.ui.shell.Panel
import io.github.tom039224.yhtye.ui.shell.PanelShell
import io.github.tom039224.yhtye.ui.shell.PanelShellState
import io.github.tom039224.yhtye.ui.shell.rememberPanelShellState
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 3 パネルの骨格: スワイプ・タップ・戻るでの開閉、端のスワイプ、横にスクロールできる要素の優先。 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w400dp-h800dp")
class PanelShellTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private lateinit var shell: PanelShellState

    /** 左パネルの幅の上限 (テストが途中で変える)。 */
    private var leftCap by mutableStateOf(Dp.Infinity)

    /** 画面の幅 (px)。 */
    private val width get() = rule.onRoot().fetchSemanticsNode().size.width.toFloat()
    private val height get() = rule.onRoot().fetchSemanticsNode().size.height.toFloat()

    private fun setContent(center: @androidx.compose.runtime.Composable () -> Unit = { Text("center") }) {
        rule.setContent {
            YhtyeTheme(darkTheme = true) {
                shell = rememberPanelShellState()
                PanelShell(
                    state = shell,
                    gestureInsets = WindowInsets(left = EDGE, right = EDGE),
                    leftMaxWidth = leftCap,
                    left = { Text("left content") },
                    right = { Text("right content") },
                    center = center,
                )
            }
        }
        rule.waitForIdle()
    }

    /** 開いたパネルの幅 (px)。 */
    private val panelWidth get() = width - with(rule.density) { PEEK.toPx() }

    /** [fromX] から、タッチスロップを越えてパネルの幅 ([of]) の [pulledShare] だけ横へ動かして離す (速さは [dpPerSecond])。 */
    private fun dragPanel(
        fromX: Float,
        pulledShare: Float,
        leftward: Boolean,
        dpPerSecond: Float = SLOW_DP_PER_SECOND,
        of: Float = panelWidth,
    ) {
        val pulled = of * pulledShare
        val y = height / 2
        rule.onRoot().performTouchInput { dragFrom(Offset(fromX, y), pulled, leftward, dpPerSecond) }
        rule.waitForIdle()
    }

    private fun swipeX(fromX: Float, toX: Float, durationMillis: Long = 300, y: Float = height / 2) {
        rule.onRoot().performTouchInput { swipe(Offset(fromX, y), Offset(toX, y), durationMillis) }
        rule.waitForIdle()
    }

    @Test
    fun `swiping right from the center opens the left panel and a tap on the center's rest closes it`() {
        setContent()
        rule.onNodeWithText("left content").assertDoesNotExist()

        swipeX(width * 0.2f, width * 0.9f)

        assertEquals(Panel.Left, shell.open)
        rule.onNodeWithText("left content").assertExists()
        // 中央の残り (右端の 64dp。覆いの左端) をタップ
        rule.onNodeWithTag("panel-shield").performTouchInput { click(Offset(10f, centerY)) }
        rule.waitForIdle()
        assertNull(shell.open)
        rule.onNodeWithText("left content").assertDoesNotExist()
    }

    @Test
    fun `swiping left from the center opens the right panel`() {
        setContent()
        swipeX(width * 0.8f, width * 0.1f)
        assertEquals(Panel.Right, shell.open)
        rule.onNodeWithText("right content").assertExists()
    }

    @Test
    fun `a short slow drag goes back and a quick flick opens`() {
        setContent()
        swipeX(width * 0.3f, width * 0.45f, durationMillis = 1_000)
        assertNull(shell.open)

        swipeX(width * 0.3f, width * 0.45f, durationMillis = 60)
        assertEquals(Panel.Left, shell.open)
    }

    // 離す位置のしきい値はパネルの幅の 35%、フリックは 245dp/s (T-72。それまでは 50% と 350dp/s)

    @Test
    fun `a slow drag opens the left panel just past 35 percent of its width and not just short of it`() {
        setContent()
        dragPanel(width * 0.1f, pulledShare = 0.30f, leftward = false)
        assertNull(shell.open)
        assertEquals(0f, shell.offset)

        dragPanel(width * 0.1f, pulledShare = 0.40f, leftward = false)
        assertEquals(Panel.Left, shell.open)
        assertEquals(panelWidth, shell.offset)
    }

    @Test
    fun `a slow drag opens the right panel just past 35 percent of its width and not just short of it`() {
        setContent()
        dragPanel(width * 0.9f, pulledShare = 0.30f, leftward = true)
        assertNull(shell.open)

        dragPanel(width * 0.9f, pulledShare = 0.40f, leftward = true)
        assertEquals(Panel.Right, shell.open)
        assertEquals(-panelWidth, shell.offset)
    }

    @Test
    fun `a slow drag closes an open panel just past 35 percent of its width and not just short of it`() {
        setContent()
        rule.runOnUiThread { shell.show(Panel.Left) }
        rule.waitForIdle()
        assertEquals(panelWidth, shell.offset)

        // 開いているパネルの上から左へ
        dragPanel(panelWidth * 0.8f, pulledShare = 0.30f, leftward = true)
        assertEquals(Panel.Left, shell.open)
        assertEquals(panelWidth, shell.offset)

        dragPanel(panelWidth * 0.8f, pulledShare = 0.40f, leftward = true)
        assertNull(shell.open)
        assertEquals(0f, shell.offset)
    }

    @Test
    fun `a short flick opens a panel above 245 dp per second and goes back below it`() {
        setContent()
        // 35% に届かない短さ (20%)。速さだけで決まる
        dragPanel(width * 0.1f, pulledShare = 0.20f, leftward = false, dpPerSecond = 180f)
        assertNull(shell.open)

        dragPanel(width * 0.1f, pulledShare = 0.20f, leftward = false, dpPerSecond = 330f)
        assertEquals(Panel.Left, shell.open)
    }

    @Test
    fun `one swipe moves one step only`() {
        setContent()
        swipeX(width * 0.2f, width * 0.9f)
        assertEquals(Panel.Left, shell.open)

        // 左パネルから右パネルまで一気に: 中央で止まる
        swipeX(width * 0.9f, 1f, durationMillis = 80)
        assertNull(shell.open)
    }

    @Test
    fun `a swipe that starts at the screen edge is left to the system back gesture`() {
        setContent()
        swipeX(5f, width * 0.9f)
        assertNull(shell.open)
        swipeX(width - 5f, width * 0.1f)
        assertNull(shell.open)
    }

    @Test
    fun `a vertical swipe does not move the panels`() {
        setContent {
            LazyColumn(Modifier.fillMaxSize()) { items(100) { Text("row $it") } }
        }
        rule.onRoot().performTouchInput { swipe(Offset(width * 0.3f, height * 0.8f), Offset(width * 0.45f, height * 0.2f), 300) }
        rule.waitForIdle()
        assertNull(shell.open)
        assertEquals(0f, shell.offset)
    }

    @Test
    fun `a code block that overflows scrolls itself instead of opening a panel`() {
        val longLine = "val x = " + "veryLongIdentifier + ".repeat(40)
        setContent {
            Box(Modifier.fillMaxSize()) { Markdown("```kotlin\n$longLine\n```") }
        }
        val code = rule.onNodeWithTag("code-scroll")
        val bounds = code.fetchSemanticsNode().boundsInRoot
        val y = bounds.center.y

        swipeX(width * 0.8f, width * 0.2f, y = y)

        assertNull(shell.open)
        val range = code.fetchSemanticsNode().config[SemanticsProperties.HorizontalScrollAxisRange]
        assertTrue(range.value() > 0f, "the code block scrolled")
        // 端まで行っていない向き (右) も、ブロックの上ではパネルを開かない (余りをパネルに渡さない)
        swipeX(width * 0.2f, width * 0.9f, y = y)
        swipeX(width * 0.2f, width * 0.9f, y = y)
        assertNull(shell.open)
    }

    @Test
    fun `a code block that fits does not keep the swipe`() {
        setContent {
            Box(Modifier.fillMaxSize()) { Markdown("```\nshort\n```") }
        }
        val y = rule.onNodeWithTag("code-scroll").fetchSemanticsNode().boundsInRoot.center.y
        swipeX(width * 0.2f, width * 0.9f, y = y)
        assertEquals(Panel.Left, shell.open)
    }

    @Test
    fun `back closes the open panel one step at a time`() {
        setContent {
            Column {
                Button(onClick = { shell.show(Panel.Right) }, modifier = Modifier.testTag("open")) { Text("open") }
            }
        }
        rule.onNodeWithTag("open").performClick()
        rule.waitForIdle()
        assertEquals(Panel.Right, shell.open)

        rule.runOnUiThread { rule.activity.onBackPressedDispatcher.onBackPressed() }
        rule.waitForIdle()
        assertNull(shell.open)
        rule.onNodeWithText("right content").assertDoesNotExist()
        // 中央では戻るは画面が扱わない (アプリを閉じる)
        assertTrue(!rule.activity.onBackPressedDispatcher.hasEnabledCallbacks())
    }

    // ---- 左パネルの幅の上限 (leftMaxWidth。ダッシュボードはレールの幅で開く) ----

    private fun px(dp: Dp) = with(rule.density) { dp.toPx() }

    private val centerLeft get() = rule.onNodeWithTag("panel-center").fetchSemanticsNode().boundsInRoot.left

    @Test
    fun `a cap on the left panel stops the center at that width, with the right panel at its full width`() {
        leftCap = CAP
        setContent()
        swipeX(width * 0.1f, width * 0.9f)
        assertEquals(Panel.Left, shell.open)
        assertEquals(px(CAP), shell.offset)
        assertEquals(px(CAP), centerLeft)
        rule.onNodeWithText("left content").assertExists()

        // 右パネルは今までの幅
        swipeX(width * 0.9f, width * 0.1f, durationMillis = 80)
        assertNull(shell.open)
        swipeX(width * 0.9f, width * 0.1f)
        assertEquals(Panel.Right, shell.open)
        assertEquals(-panelWidth, shell.offset)
    }

    @Test
    fun `the left panel opens from the button to the cap and closes in one swipe`() {
        leftCap = CAP
        setContent()
        rule.runOnUiThread { shell.show(Panel.Left) }
        rule.waitForIdle()
        assertEquals(px(CAP), shell.offset)

        // 1 回の左向きのスワイプで中央まで戻る (途中で止まらない)
        swipeX(px(CAP) * 0.9f, 10f, durationMillis = 120)
        assertNull(shell.open)
        assertEquals(0f, shell.offset)
        rule.onNodeWithText("left content").assertDoesNotExist()
    }

    @Test
    fun `a cap wider than the screen allows has no effect`() {
        leftCap = 1_000.dp
        setContent()
        swipeX(width * 0.1f, width * 0.9f)
        assertEquals(Panel.Left, shell.open)
        assertEquals(panelWidth, shell.offset)
        assertEquals(panelWidth, centerLeft)
    }

    @Test
    fun `a slow drag opens and closes the capped panel at 35 percent of the cap, not of the screen`() {
        leftCap = CAP
        setContent()
        val cap = px(CAP)
        dragPanel(width * 0.1f, pulledShare = 0.30f, leftward = false, of = cap)
        assertNull(shell.open)
        assertEquals(0f, shell.offset)

        dragPanel(width * 0.1f, pulledShare = 0.40f, leftward = false, of = cap)
        assertEquals(Panel.Left, shell.open)
        assertEquals(cap, shell.offset)

        dragPanel(cap * 0.8f, pulledShare = 0.30f, leftward = true, of = cap)
        assertEquals(Panel.Left, shell.open)
        assertEquals(cap, shell.offset)

        dragPanel(cap * 0.8f, pulledShare = 0.40f, leftward = true, of = cap)
        assertNull(shell.open)
        assertEquals(0f, shell.offset)
    }

    @Test
    fun `a drag toward the open side over a capped panel does nothing`() {
        leftCap = CAP
        setContent()
        rule.runOnUiThread { shell.show(Panel.Left) }
        rule.waitForIdle()
        dragPanel(px(CAP) * 0.4f, pulledShare = 1f, leftward = false)
        assertEquals(Panel.Left, shell.open)
        assertEquals(px(CAP), shell.offset)
        dragPanel(px(CAP) * 0.4f, pulledShare = 0.2f, leftward = false, dpPerSecond = 600f)
        assertEquals(Panel.Left, shell.open)
        assertEquals(px(CAP), shell.offset)
    }

    @Test
    fun `changing the cap while the left panel is open moves the center to the new width with an animation`() {
        setContent()
        rule.runOnUiThread { shell.show(Panel.Left) }
        rule.waitForIdle()
        assertEquals(panelWidth, shell.offset)

        rule.mainClock.autoAdvance = false
        rule.runOnUiThread { leftCap = CAP }
        // 状態の変更が合成に届き、効果が動き出すまでフレームを進める
        repeat(3) {
            shadowOf(Looper.getMainLooper()).idle()
            rule.mainClock.advanceTimeByFrame()
        }
        rule.mainClock.advanceTimeBy(100)
        val midway = shell.offset
        assertTrue(midway < panelWidth && midway > px(CAP), "midway $midway between ${px(CAP)} and $panelWidth")
        rule.mainClock.autoAdvance = true
        rule.waitForIdle()
        assertEquals(Panel.Left, shell.open)
        assertEquals(px(CAP), shell.offset)

        // 戻すと全幅へ
        rule.runOnUiThread { leftCap = Dp.Infinity }
        rule.waitForIdle()
        assertEquals(panelWidth, shell.offset)
    }

    @Test
    fun `changing the cap while the panels are closed does not open anything`() {
        setContent()
        rule.runOnUiThread { leftCap = CAP }
        rule.waitForIdle()
        assertNull(shell.open)
        assertEquals(0f, shell.offset)
        rule.onNodeWithText("left content").assertDoesNotExist()
        // 次に開くときは新しい幅
        swipeX(width * 0.1f, width * 0.9f)
        assertEquals(px(CAP), shell.offset)
    }

    @Test
    fun `changing the cap while the right panel is open leaves it where it is`() {
        setContent()
        rule.runOnUiThread { shell.show(Panel.Right) }
        rule.waitForIdle()
        rule.runOnUiThread { leftCap = CAP }
        rule.waitForIdle()
        assertEquals(Panel.Right, shell.open)
        assertEquals(-panelWidth, shell.offset)
    }

    private companion object {
        val EDGE = 24.dp
        val CAP = 232.dp
    }
}

/** 幅の広い端末では左右のパネルを常時表示し、ジェスチャでは動かない。 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w1000dp-h800dp")
class PanelShellWideTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun `wide screens show both panels`() {
        lateinit var shell: PanelShellState
        rule.setContent {
            YhtyeTheme {
                shell = rememberPanelShellState()
                PanelShell(shell, left = { Text("left content") }, right = { Text("right content") }, center = { Text("center") })
            }
        }
        rule.waitForIdle()
        rule.onNodeWithText("left content").assertExists()
        rule.onNodeWithText("right content").assertExists()
        rule.onNodeWithText("center").assertExists()
        assertTrue(shell.wide)
    }
}
