package io.github.tom039224.yhtye.ui

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.compose.ui.unit.dp
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.Help
import io.github.tom039224.yhtye.shared.api.generated.HelpKind
import io.github.tom039224.yhtye.shared.api.generated.HelpSource
import io.github.tom039224.yhtye.shared.api.generated.HelpState
import io.github.tom039224.yhtye.shared.api.generated.Role
import io.github.tom039224.yhtye.shared.api.generated.Snapshot
import io.github.tom039224.yhtye.shared.api.generated.StepStatus
import io.github.tom039224.yhtye.shared.api.generated.Task
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** 記録の終わりのスナップショットを、動いているグループ (と過去のグループ) にしたもの。 */
fun activeGroup(h: MainHarness, groupStatus: GroupStatus = GroupStatus.Active, detail: String? = null): Snapshot {
    val end = h.recording.end
    val base = end.state.tasks.first()
    fun task(id: String, title: String, status: TaskStatus, running: Boolean, depends: List<String> = emptyList()): Task = base.copy(
        id = id,
        title = title,
        status = status,
        current = 0,
        dependsOn = depends,
        steps = base.steps.mapIndexed { i, s ->
            s.copy(status = if (i == 0 && running) StepStatus.Running else StepStatus.Pending, result = null, verdict = null)
        },
    )
    val group = end.state.groups.first().copy(status = groupStatus, detail = detail)
    val past = end.state.groups.first().copy(id = "G-0", title = "earlier work")
    val help = Help(
        id = "H-1",
        task = "T-2",
        step = 0,
        kind = HelpKind.Blocked,
        message = "注文スキーマが v2 と v3 で混在しています",
        source = HelpSource.Agent(Role.Implementer),
        state = HelpState.Open,
        agentLost = false,
    )
    return end.copy(
        state = end.state.copy(
            groups = listOf(past, group),
            tasks = listOf(
                base.copy(id = "T-0", group = "G-0", title = "earlier task"),
                task("T-1", base.title, TaskStatus.Running, running = true),
                task("T-2", "legacy totals", TaskStatus.Handling, running = true),
                task("T-3", "address form", TaskStatus.Pending, running = false, depends = listOf("T-1")),
            ),
            helps = listOf(help),
        ),
    )
}

/**
 * 右パネル: PC の TaskColumn と同じ情報、help は状態の表示だけ、タスクの行の右→左スワイプでメンション
 * (閾値未満は戻る・左→右はパネルを閉じる)、■ の確認と中止、グループの中止・マージの再試行、過去のグループ。
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w400dp-h800dp")
class RightPanelTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val h = MainHarness(rule)

    @After
    fun tearDown() = h.close()

    /** タスクのカードの上を横に動かす ([fromX] / [toX] はカードの幅に対する割合)。 */
    private fun swipeCard(task: String, fromX: Float, toX: Float, durationMillis: Long = 300) {
        rule.onNodeWithTag("task-$task").performTouchInput {
            swipe(Offset(width * fromX, centerY), Offset(width * toX, centerY), durationMillis)
        }
        rule.waitForIdle()
    }

    @Test
    fun `cards show the group and tasks like the PC and help only as a state`() {
        h.open(activeGroup(h))
        h.openRight()
        rule.onNodeWithTag("group-title").assertTextEquals("greeting")
        rule.onNodeWithTag("group-status-G-1", useUnmergedTree = true).assertTextEquals("進行中")
        rule.onNodeWithTag("group-count-G-1", useUnmergedTree = true).assertTextEquals("0/3")
        rule.onNode(hasContentDescription("1 件 対処中")).assertExists()
        rule.onNode(hasContentDescription("グループブランチ ← マージ先 (base)")).assertExists()
        rule.onNodeWithTag("task-status-T-1", useUnmergedTree = true).assertTextEquals("実装中")
        rule.onNodeWithTag("task-status-T-2", useUnmergedTree = true).assertTextEquals("対処中")
        rule.onNodeWithTag("task-status-T-3", useUnmergedTree = true).assertTextEquals("待機")
        // help は文を出すだけ (扱うのはオーケストレータ)。答える・判断する操作は無い
        rule.onNodeWithText("注文スキーマが v2 と v3 で混在しています", useUnmergedTree = true).assertExists()
        assertEquals(0, rule.onAllNodes(hasText("回答", substring = true)).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodes(hasText("判断", substring = true)).fetchSemanticsNodes().size)
        // 依存の待ち (PC の taskMeta)
        rule.onNode(hasContentDescription("T-1 の完了待ち")).assertExists()
    }

    @Test
    fun `swiping a task row from right to left quotes it and returns to the center`() {
        h.open(activeGroup(h))
        h.openRight()
        swipeCard("T-1", 0.85f, 0.15f)
        h.settle { h.exists("mention-T-1") }
        assertFalse(h.exists("right-panel"))
    }

    @Test
    fun `a short slow swipe springs back without quoting`() {
        h.open(activeGroup(h))
        h.openRight()
        swipeCard("T-1", 0.85f, 0.72f, durationMillis = 1_000)
        h.settle { true }
        assertFalse(h.exists("mention-T-1"))
        assertTrue(h.exists("right-panel"))
    }

    @Test
    fun `a quick short flick quotes too`() {
        h.open(activeGroup(h))
        h.openRight()
        swipeCard("T-1", 0.85f, 0.6f, durationMillis = 60)
        h.settle { h.exists("mention-T-1") }
    }

    @Test
    fun `a left to right swipe on a row closes the panel instead of quoting`() {
        h.open(activeGroup(h))
        h.openRight()
        swipeCard("T-1", 0.1f, 0.9f)
        h.settle { !h.exists("right-panel") }
        assertFalse(h.exists("mention-T-1"))
    }

    @Test
    fun `a row swipe that starts on the screen edge is left to the system`() {
        h.open(activeGroup(h), insets = WindowInsets(left = 24.dp, right = 24.dp))
        h.openRight()
        val card = rule.onNodeWithTag("task-T-1").fetchSemanticsNode().boundsInRoot
        val edge = rule.onRoot().fetchSemanticsNode().size.width - 5f
        rule.onRoot().performTouchInput { swipe(Offset(edge, card.center.y), Offset(edge - card.width * 0.7f, card.center.y), 300) }
        rule.waitForIdle()
        h.settle { true }
        assertFalse(h.exists("mention-T-1"))
    }

    @Test
    fun `the stop button asks first and then cancels the task`() {
        h.open(activeGroup(h))
        h.openRight()
        rule.onNodeWithTag("cancel-task-T-1").performClick()
        rule.onNodeWithText("T-1 を中止しますか?").assertExists()
        rule.onNodeWithTag("cancel-dismiss").performClick()
        rule.waitForIdle()
        assertTrue(h.core.callsOf<ApiCommand.CancelTask>().isEmpty())

        rule.onNodeWithTag("cancel-task-T-1").performClick()
        rule.onNodeWithTag("cancel-confirm").performClick()
        h.settle { h.core.callsOf<ApiCommand.CancelTask>().isNotEmpty() }
        assertEquals("T-1", h.core.callsOf<ApiCommand.CancelTask>().single().task)
    }

    @Test
    fun `past groups are folded and finished tasks have no stop button`() {
        h.open(activeGroup(h))
        h.openRight()
        assertFalse(h.exists("task-T-0"))
        rule.onNodeWithTag("past-groups").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("right-panel").performScrollToNode(hasTestTag("task-T-0"))
        rule.onNodeWithTag("task-T-0").assertExists()
        assertFalse(h.exists("cancel-task-T-0"))
        assertFalse(h.exists("cancel-group-G-0"))
    }

    @Test
    fun `the group stop asks and cancels the group`() {
        h.open(activeGroup(h))
        h.openRight()
        rule.onNodeWithTag("cancel-group-G-1").performClick()
        rule.onNodeWithText("G-1 を中止しますか?").assertExists()
        rule.onNodeWithTag("cancel-confirm").performClick()
        h.settle { h.core.callsOf<ApiCommand.CancelGroup>().isNotEmpty() }
        assertEquals("G-1", h.core.callsOf<ApiCommand.CancelGroup>().single().group)
    }

    @Test
    fun `a merge blocked group shows the reason and retries the merge`() {
        h.open(activeGroup(h, GroupStatus.MergeBlocked, detail = "conflict in calc.ts"))
        h.openRight()
        rule.onNodeWithTag("merge-blocked-G-1").assertTextEquals("base ブランチへのマージが保留されています: conflict in calc.ts")
        rule.onNodeWithTag("group-status-G-1", useUnmergedTree = true).assertTextEquals("マージ保留")
        rule.onNodeWithTag("retry-merge-G-1").performClick()
        h.settle { h.core.callsOf<ApiCommand.RetryGroupMerge>().isNotEmpty() }
        assertEquals("G-1", h.core.callsOf<ApiCommand.RetryGroupMerge>().single().group)
    }

    @Test
    fun `a chat without groups says so`() {
        h.open(h.recording.end.copy(state = h.recording.end.state.copy(groups = emptyList(), tasks = emptyList())))
        h.openRight()
        rule.onNodeWithText("グループはまだありません", useUnmergedTree = true).assertExists()
    }
}
