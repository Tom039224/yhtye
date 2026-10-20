package io.github.tom039224.yhtye.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.click
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.ui.panels.MainView
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
 * レール (PC の IconRail・RailProjects): 開いているプロジェクトのドット、タップでの切り替え、もう 1 回のスワイプでの展開と
 * 逆のスワイプでの畳み (パネルの開閉のスワイプを壊さない)。
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w400dp-h800dp")
class RailTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val h = MainHarness(rule)

    @After
    fun tearDown() = h.close()

    private val other = ProjectInfo(id = "other", name = "other-repo", path = "/tmp/other", open = false, running = false, awake = false)
    private val busy = ProjectInfo(id = "busy", name = "busy-repo", path = "/tmp/busy", open = true, running = true, awake = true)

    /** 畳んだ幅から広がった幅までの道のり (232dp - 56dp)。 */
    private val distance get() = with(rule.density) { 176.dp.toPx() }

    private fun stateOf(tag: String) =
        rule.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().config[SemanticsProperties.StateDescription]

    private fun railWidthDp(): Float = with(rule.density) { rule.onNodeWithTag("rail").fetchSemanticsNode().size.width.toDp().value }

    /** 開いた左パネルの上で、道のりの [share] だけ横へゆっくり動かす。 */
    private fun dragOnPanel(share: Float, leftward: Boolean = false) {
        rule.onRoot().performTouchInput {
            val from = Offset(with(rule.density) { (if (leftward) 280.dp else 120.dp).toPx() }, height / 2f)
            dragFrom(from, distance * share, leftward)
        }
        h.settleFor()
    }

    private fun expanded(): Boolean = h.exists("rail-outside")

    @Test
    fun `the rail lists the open projects as dots, lit while work is on`() {
        h.open { projects = listOf(PROJECT, other, busy) }
        h.openLeft()
        assertTrue(h.exists("rail-project-${PROJECT.id}"))
        assertTrue(h.exists("rail-project-${busy.id}"))
        // 開いていないプロジェクトはレールに無い (ダッシュボードにはある)
        assertFalse(h.exists("rail-project-${other.id}"))
        assertEquals("実行中", stateOf("rail-dot-${busy.id}"))
        assertTrue(stateOf("rail-dot-${PROJECT.id}") != "実行中")
        // 畳んでいればドットだけ (名前は出ない)
        rule.onNode(hasText(busy.name) and hasAnyAncestor(hasTestTag("rail")), useUnmergedTree = true).assertDoesNotExist()
        assertEquals(56f, railWidthDp(), 0.5f)
        // 表示中のプロジェクトはコーディングのビューでアクティブ
        rule.onNodeWithTag("rail-project-${PROJECT.id}").assertIsSelected()
        rule.onNodeWithTag("rail-work").assertIsSelected()
    }

    @Test
    fun `a tap on a dot switches to that project and the work view`() {
        h.open(view = MainView.Dashboard) { projects = listOf(PROJECT, busy) }
        assertTrue(h.exists("dashboard"))
        h.openLeft()
        rule.onNodeWithTag("rail-dashboard").assertIsSelected()
        rule.onNodeWithTag("rail-project-${busy.id}").performClick()
        h.settle { h.sync.state.value.project?.info?.id == busy.id }
        assertEquals(listOf(PROJECT.path, busy.path), h.core.callsOf<ApiCommand.OpenProject>().map { it.path })
        assertFalse(h.exists("dashboard"))
        rule.onNodeWithTag("rail-work").assertIsSelected()
        rule.onNodeWithTag("rail-project-${busy.id}").assertIsSelected()
    }

    @Test
    fun `a tap on the project on screen does not open it again`() {
        h.open(view = MainView.Dashboard)
        h.openLeft()
        rule.onNodeWithTag("rail-project-${PROJECT.id}").performClick()
        h.settleFor()
        assertEquals(1, h.core.callsOf<ApiCommand.OpenProject>().size)
        assertFalse(h.exists("dashboard"))
    }

    @Test
    fun `one more swipe toward the left side expands the rail with the names, the opposite one folds it`() {
        h.open { projects = listOf(PROJECT, busy) }
        h.openLeft()
        dragOnPanel(0.5f)
        assertTrue(expanded())
        assertEquals(232f, railWidthDp(), 0.5f)
        rule.onNode(hasText(busy.name) and hasAnyAncestor(hasTestTag("rail")), useUnmergedTree = true).assertExists()
        rule.onNodeWithText("ダッシュボード").assertExists()
        rule.onNodeWithText("プロジェクト").assertExists()
        // パネルは開いたまま
        assertTrue(h.exists("panel-shield"))

        // 逆の向きは、まず畳むだけ (パネルは閉じない)
        dragOnPanel(0.5f, leftward = true)
        assertFalse(expanded())
        assertEquals(56f, railWidthDp(), 0.5f)
        rule.onNode(hasText(busy.name) and hasAnyAncestor(hasTestTag("rail")), useUnmergedTree = true).assertDoesNotExist()
        assertTrue(h.exists("panel-shield"))

        // もう 1 回でパネルが閉じる (今までどおり)
        h.dragScreen(0.5f, leftward = true)
        h.settleFor()
        assertFalse(h.exists("panel-left"))
    }

    @Test
    fun `a short slow swipe does not expand the rail and a quick flick does`() {
        h.open()
        h.openLeft()
        dragOnPanel(0.2f)
        assertFalse(expanded())
        assertEquals(56f, railWidthDp(), 0.5f)

        rule.onRoot().performTouchInput {
            dragFrom(Offset(with(rule.density) { 120.dp.toPx() }, height / 2f), distance * 0.2f, leftward = false, dpPerSecond = 600f)
        }
        h.settleFor()
        assertTrue(expanded())
    }

    @Test
    fun `swipes still open and close the left panel while the rail stays folded`() {
        h.open()
        h.dragScreen(0.6f)
        h.settleFor()
        assertTrue(h.exists("panel-left"))
        assertFalse(expanded())
        h.dragScreen(0.6f, leftward = true)
        h.settleFor()
        assertFalse(h.exists("panel-left"))
    }

    @Test
    fun `choosing a project in the expanded rail folds it`() {
        h.open(view = MainView.Dashboard) { projects = listOf(PROJECT, busy) }
        h.openLeft()
        dragOnPanel(0.5f)
        assertTrue(expanded())
        rule.onNodeWithTag("rail-project-${busy.id}").performClick()
        h.settle { h.sync.state.value.project?.info?.id == busy.id }
        h.settleFor()
        assertFalse(expanded())
        assertTrue(h.exists("panel-left"))
        assertFalse(h.exists("dashboard"))
    }

    @Test
    fun `a tap beside the expanded rail folds it`() {
        h.open()
        h.openLeft()
        dragOnPanel(0.5f)
        rule.onNodeWithTag("rail-outside").performTouchInput { click(Offset(width - 20f, height / 2f)) }
        h.settleFor()
        assertFalse(expanded())
        assertTrue(h.exists("panel-left"))
    }

    @Test
    fun `back folds the expanded rail before it closes the panel`() {
        h.open()
        h.openLeft()
        dragOnPanel(0.5f)
        rule.runOnUiThread { rule.activity.onBackPressedDispatcher.onBackPressed() }
        h.settleFor()
        assertFalse(expanded())
        assertTrue(h.exists("panel-left"))
        rule.runOnUiThread { rule.activity.onBackPressedDispatcher.onBackPressed() }
        h.settleFor()
        assertFalse(h.exists("panel-left"))
    }

    @Test
    fun `the rail folds when the panel closes`() {
        h.open()
        h.openLeft()
        dragOnPanel(0.5f)
        rule.onNodeWithTag("panel-shield").performTouchInput { click(Offset(10f, height / 2f)) }
        h.settleFor()
        assertFalse(h.exists("panel-left"))
        h.openLeft()
        h.settleFor()
        assertFalse(expanded())
    }

    @Test
    fun `the rail is reached in every view and the dashboard button switches to it`() {
        h.open()
        h.openLeft()
        rule.onNodeWithTag("rail-dashboard").performClick()
        h.settleFor()
        assertTrue(h.exists("dashboard"))
        rule.onNodeWithTag("rail-dashboard").assert(SemanticsMatcher.expectValue(SemanticsProperties.Selected, true))
    }
}
