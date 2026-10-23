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
import androidx.compose.ui.unit.Dp
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
 * 逆のスワイプでの畳み (パネルの開閉のスワイプを壊さない)。ダッシュボードでは 1 回のスワイプで広がったレールまで開き、
 * そのレールは広げたまま固定する。
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

    /** 中央の左端 (dp)。左パネルが開いていればパネルの右端。 */
    private fun centerLeftDp(): Float = with(rule.density) { rule.onNodeWithTag("panel-center").fetchSemanticsNode().boundsInRoot.left.toDp().value }

    /** 画面全体の [fromDp] (dp) から、タッチスロップを越えて [pulledDp] (dp) だけ横へゆっくり動かして離す。 */
    private fun dragAt(fromDp: Float, pulledDp: Float, leftward: Boolean) {
        rule.onRoot().performTouchInput {
            dragFrom(Offset(with(rule.density) { fromDp.dp.toPx() }, height / 2f), with(rule.density) { pulledDp.dp.toPx() }, leftward)
        }
        h.settleFor()
    }

    /** レールにプロジェクトの名前が出ている (広がっている)。 */
    private fun railShowsNames(): Boolean = rule.onAllNodes(hasText(busy.name) and hasAnyAncestor(hasTestTag("rail")), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()

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
        assertEquals(56f, railWidthDp(), 0.5f)
        assertEquals(PANEL_FULL_DP, centerLeftDp(), 0.5f)
        h.dragScreen(0.6f, leftward = true)
        h.settleFor()
        assertFalse(h.exists("panel-left"))
    }

    @Test
    fun `choosing a project in the expanded rail folds it`() {
        h.open { projects = listOf(PROJECT, busy) }
        h.openLeft()
        dragOnPanel(0.5f)
        assertTrue(expanded())
        rule.onNodeWithTag("rail-project-${busy.id}").performClick()
        h.settle { h.sync.state.value.project?.info?.id == busy.id }
        h.settleFor()
        assertFalse(expanded())
        assertEquals(56f, railWidthDp(), 0.5f)
        assertTrue(h.exists("panel-left"))
    }

    @Test
    fun `choosing a project in the pinned dashboard rail folds it and the panel grows to the full width`() {
        h.open(view = MainView.Dashboard) { projects = listOf(PROJECT, busy) }
        h.openLeft()
        assertEquals(232f, railWidthDp(), 0.5f)
        rule.onNodeWithTag("rail-project-${busy.id}").performClick()
        h.settle { h.sync.state.value.project?.info?.id == busy.id }
        h.settleFor()
        assertFalse(h.exists("dashboard"))
        assertEquals(56f, railWidthDp(), 0.5f)
        assertEquals(PANEL_FULL_DP, centerLeftDp(), 0.5f)
        assertTrue(h.exists("panel-left"))
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

    // ---- ダッシュボード: 1 回のスワイプで広がったレールまで開き、レールは広げたまま固定する ----

    @Test
    fun `one swipe from the dashboard opens the left panel straight to the expanded rail`() {
        h.open(view = MainView.Dashboard) { projects = listOf(PROJECT, busy) }
        assertTrue(h.exists("dashboard"))
        h.dragScreen(0.6f)
        h.settleFor()
        assertTrue(h.exists("panel-left"))
        assertEquals(232f, railWidthDp(), 0.5f)
        assertTrue(railShowsNames())
        listOf("ダッシュボード", "プロジェクト", "コーディング").forEach { label ->
            rule.onNode(hasText(label) and hasAnyAncestor(hasTestTag("rail")), useUnmergedTree = true).assertExists()
        }
        // 中央はレールの右端で止まる (間が空かない・その先は 64dp を残す幅まで行かない)
        assertEquals(232f, centerLeftDp(), 0.5f)
        // 固定したレールには外のタップの受け手も無い
        assertFalse(expanded())
    }

    @Test
    fun `the menu button opens the dashboard panel to the same expanded rail`() {
        h.open(view = MainView.Dashboard) { projects = listOf(PROJECT, busy) }
        h.openLeft()
        h.settleFor()
        assertEquals(232f, railWidthDp(), 0.5f)
        assertTrue(railShowsNames())
        assertEquals(232f, centerLeftDp(), 0.5f)
        // 使用量の欄もパネルに収まる
        assertTrue(h.exists("usage"))
    }

    @Test
    fun `one swipe back from the dashboard panel returns to the dashboard with no stop in between`() {
        h.open(view = MainView.Dashboard)
        h.dragScreen(0.6f)
        h.settleFor()
        assertTrue(h.exists("panel-left"))
        h.dragScreen(0.5f, leftward = true)
        h.settleFor()
        assertFalse(h.exists("panel-left"))
        assertTrue(h.exists("dashboard"))
        assertEquals(0f, centerLeftDp(), 0.5f)
    }

    @Test
    fun `a swipe back that starts on the pinned rail also returns to the dashboard in one step`() {
        h.open(view = MainView.Dashboard) { projects = listOf(PROJECT, busy) }
        h.openLeft()
        dragAt(fromDp = 120f, pulledDp = 120f, leftward = true)
        assertFalse(h.exists("panel-left"))
        assertTrue(h.exists("dashboard"))
    }

    @Test
    fun `back returns from the pinned rail to the dashboard in one step`() {
        h.open(view = MainView.Dashboard)
        h.openLeft()
        rule.runOnUiThread { rule.activity.onBackPressedDispatcher.onBackPressed() }
        h.settleFor()
        assertFalse(h.exists("panel-left"))
        assertTrue(h.exists("dashboard"))
        assertTrue(!rule.activity.onBackPressedDispatcher.hasEnabledCallbacks())
    }

    @Test
    fun `a tap beside the pinned rail closes the panel`() {
        h.open(view = MainView.Dashboard)
        h.openLeft()
        rule.onNodeWithTag("panel-shield").performTouchInput { click(Offset(10f, height / 2f)) }
        h.settleFor()
        assertFalse(h.exists("panel-left"))
    }

    @Test
    fun `the dashboard swipe commits at 35 percent of the 232dp the panel opens to`() {
        h.open(view = MainView.Dashboard)
        // 232dp の 35% は 81.2dp: 手前では戻り、超えれば開く
        dragAt(fromDp = 40f, pulledDp = 232f * 0.30f, leftward = false)
        assertFalse(h.exists("panel-left"))
        dragAt(fromDp = 40f, pulledDp = 232f * 0.40f, leftward = false)
        assertTrue(h.exists("panel-left"))
        assertEquals(232f, centerLeftDp(), 0.5f)
        // 閉じるときも同じ割合
        dragAt(fromDp = 300f, pulledDp = 232f * 0.30f, leftward = true)
        assertTrue(h.exists("panel-left"))
        assertEquals(232f, centerLeftDp(), 0.5f)
        dragAt(fromDp = 300f, pulledDp = 232f * 0.40f, leftward = true)
        assertFalse(h.exists("panel-left"))
    }

    @Test
    fun `dragging over the pinned rail does nothing`() {
        h.open(view = MainView.Dashboard) { projects = listOf(PROJECT, busy) }
        h.openLeft()
        // 右へ (広げる向き): レールもパネルもそのまま
        dragAt(fromDp = 100f, pulledDp = 120f, leftward = false)
        assertTrue(h.exists("panel-left"))
        assertEquals(232f, railWidthDp(), 0.5f)
        assertEquals(232f, centerLeftDp(), 0.5f)
        // 速いはじきでも (これ以上広がらない)
        rule.onRoot().performTouchInput {
            dragFrom(Offset(with(rule.density) { 100.dp.toPx() }, height / 2f), with(rule.density) { 40.dp.toPx() }, leftward = false, dpPerSecond = 800f)
        }
        h.settleFor()
        assertTrue(h.exists("panel-left"))
        assertEquals(232f, railWidthDp(), 0.5f)
        assertEquals(232f, centerLeftDp(), 0.5f)
    }

    @Test
    fun `switching between coding and the dashboard in the rail resizes the panel`() {
        h.open(view = MainView.Dashboard) { projects = listOf(PROJECT, busy) }
        h.openLeft()
        h.settleFor()
        assertEquals(232f, centerLeftDp(), 0.5f)
        assertEquals(232f, railWidthDp(), 0.5f)

        // コーディングへ: レールを畳みながらパネルが全幅に広がる
        rule.onNodeWithTag("rail-work").performClick()
        h.settleFor()
        assertTrue(h.exists("panel-left"))
        assertEquals(56f, railWidthDp(), 0.5f)
        assertEquals(PANEL_FULL_DP, centerLeftDp(), 0.5f)
        rule.onNodeWithTag("current-project").assertExists()

        // ダッシュボードへ: パネルがレールの幅に縮んでレールが広がる
        rule.onNodeWithTag("rail-dashboard").performClick()
        h.settleFor()
        assertTrue(h.exists("panel-left"))
        assertEquals(232f, railWidthDp(), 0.5f)
        assertEquals(232f, centerLeftDp(), 0.5f)
        assertFalse(h.exists("current-project"))
    }

    @Test
    fun `the work view keeps the two steps, open the panel then expand the rail`() {
        h.open { projects = listOf(PROJECT, busy) }
        h.dragScreen(0.6f)
        h.settleFor()
        assertEquals(56f, railWidthDp(), 0.5f)
        assertEquals(PANEL_FULL_DP, centerLeftDp(), 0.5f)
        assertFalse(railShowsNames())
        dragOnPanel(0.5f)
        assertEquals(232f, railWidthDp(), 0.5f)
        assertEquals(PANEL_FULL_DP, centerLeftDp(), 0.5f)
        assertTrue(railShowsNames())
        // 戻りも 2 段
        dragOnPanel(0.5f, leftward = true)
        assertEquals(56f, railWidthDp(), 0.5f)
        assertTrue(h.exists("panel-left"))
        h.dragScreen(0.5f, leftward = true)
        h.settleFor()
        assertFalse(h.exists("panel-left"))
    }
}

/** 幅の広い端末 (左パネルは常時表示): ダッシュボードでもレールは固定せず、畳んだ状態からスワイプで広げる。 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w1000dp-h800dp")
class RailWideTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val h = MainHarness(rule)

    @After
    fun tearDown() = h.close()

    private fun railWidthDp(): Float = with(rule.density) { rule.onNodeWithTag("rail").fetchSemanticsNode().size.width.toDp().value }

    @Test
    fun `the dashboard rail is not pinned on a wide screen and the swipe still expands and folds it`() {
        h.open(view = MainView.Dashboard)
        h.settleFor()
        assertTrue(h.exists("panel-left"))
        assertEquals(56f, railWidthDp(), 0.5f)
        assertFalse(h.exists("rail-outside"))

        h.drag(from = 100.dp, pulled = 120.dp, leftward = false)
        assertEquals(232f, railWidthDp(), 0.5f)
        assertTrue(h.exists("rail-outside"))

        h.drag(from = 280.dp, pulled = 120.dp, leftward = true)
        assertEquals(56f, railWidthDp(), 0.5f)
        assertFalse(h.exists("rail-outside"))
    }

    private fun MainHarness.drag(from: Dp, pulled: Dp, leftward: Boolean) {
        rule.onRoot().performTouchInput {
            dragFrom(Offset(with(rule.density) { from.toPx() }, height / 2f), with(rule.density) { pulled.toPx() }, leftward)
        }
        settleFor()
    }
}

/** 幅 400dp の端末で、左パネルが全幅に開いたときの中央の左端 (400dp - 64dp)。 */
private const val PANEL_FULL_DP = 336f
