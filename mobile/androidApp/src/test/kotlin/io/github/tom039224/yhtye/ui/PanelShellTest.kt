package io.github.tom039224.yhtye.ui

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.Button
import androidx.compose.material3.Text
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
import androidx.compose.ui.unit.dp
import io.github.tom039224.yhtye.YhtyeTheme
import io.github.tom039224.yhtye.ui.conversation.Markdown
import io.github.tom039224.yhtye.ui.shell.Panel
import io.github.tom039224.yhtye.ui.shell.PanelShell
import io.github.tom039224.yhtye.ui.shell.PanelShellState
import io.github.tom039224.yhtye.ui.shell.rememberPanelShellState
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
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
                    left = { Text("left content") },
                    right = { Text("right content") },
                    center = center,
                )
            }
        }
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

    private companion object {
        val EDGE = 24.dp
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
