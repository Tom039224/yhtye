package io.github.tom039224.yhtye.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.click
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.StatsSource
import io.github.tom039224.yhtye.shared.api.generated.StatsTokens
import io.github.tom039224.yhtye.shared.api.generated.StatsTotals
import io.github.tom039224.yhtye.ui.dashboard.AGENTSVIEW_URL
import io.github.tom039224.yhtye.ui.panels.MainView
import kotlinx.coroutines.CompletableDeferred
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
 * ダッシュボード (PC の Dashboard): 最初のビュー、全プロジェクトの一覧 (タップで開く)、ホストのフォルダを辿って追加、
 * 利用統計 (期間・合計・グラフ・内訳と、読み込み中・ログ無し・利用なし・失敗)、クレジット。
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w400dp-h800dp")
class DashboardTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val h = MainHarness(rule)

    @After
    fun tearDown() = h.close()

    private val other = ProjectInfo(id = "other", name = "other-repo", path = "/tmp/other", open = false, running = false, awake = false)
    private val busy = ProjectInfo(id = "busy", name = "busy-repo", path = "/tmp/busy", open = true, running = true, awake = true)

    private fun dashboard(withProject: Boolean = true, configure: TestCore.() -> Unit = {}) =
        h.open(view = MainView.Dashboard, withProject = withProject, configure = configure)

    /** ホストの名前を読む (ping は前面にある間だけなので、一度前面にして答えを待ち、また戻す)。 */
    private fun learnHostName() {
        h.sync.setForeground(true)
        h.settle { h.sync.state.value.host.name != null }
        h.sync.setForeground(false)
        h.settleFor()
    }

    private fun onStats(tag: String) = rule.onNodeWithTag(tag, useUnmergedTree = true)

    @Test
    fun `the dashboard is the first view and lists every registered project`() {
        dashboard { projects = listOf(PROJECT, other, busy) }
        rule.onNodeWithText("ようこそ").assertExists()
        // 開いていないプロジェクトも出る
        listOf(PROJECT, other, busy).forEach { rule.onNodeWithTag("dash-project-${it.id}").assertExists() }
        rule.onNode(hasContentDescription("busy-repo を開く (実行中)")).assertExists()
        rule.onNodeWithTag("dash-project-${PROJECT.id}").assertIsSelected()
        assertEquals("実行中", rule.onNodeWithTag("dash-dot-${busy.id}", useUnmergedTree = true).fetchSemanticsNode().config[SemanticsProperties.StateDescription])
        learnHostName()
        rule.onNodeWithTag("add-project-target", useUnmergedTree = true).assertTextEquals("追加先: test-host")
    }

    @Test
    fun `a tap on a project opens it and switches to the work view`() {
        dashboard { projects = listOf(PROJECT, other) }
        rule.onNodeWithTag("dash-project-${other.id}").performClick()
        h.settle { h.sync.state.value.project?.info?.id == other.id }
        assertFalse(h.exists("dashboard"))
        assertTrue(h.exists("open-right"))
    }

    @Test
    fun `without projects the list asks to add one`() {
        dashboard(withProject = false) { projects = emptyList() }
        rule.onNodeWithTag("projects-empty").assertExists()
    }

    @Test
    fun `the work view without a project leads back to the dashboard`() {
        h.open(view = MainView.Work, withProject = false)
        rule.onNodeWithTag("no-project").assertExists()
        rule.onNodeWithTag("open-dashboard").performClick()
        rule.waitForIdle()
        assertTrue(h.exists("dashboard"))
    }

    // ---- プロジェクトを追加 (ホストのフォルダ) ----

    private fun openBrowser() {
        rule.onNodeWithTag("add-project").performClick()
        h.settle { h.exists("folder-browser") && h.sync.folders.state.value.path != null }
    }

    private fun current(path: String) {
        h.settle { h.sync.folders.state.value.path == path && !h.sync.folders.state.value.loading }
        rule.onNodeWithTag("folder-current", useUnmergedTree = true).assertTextEquals(path)
    }

    @Test
    fun `add project browses the host's folders from its home and opens the chosen one`() {
        dashboard()
        learnHostName()
        openBrowser()
        current("/home/u")
        rule.onNodeWithText("test-host 上のフォルダを選択").assertExists()
        // 隠しフォルダは出ない。git リポジトリには印
        assertFalse(h.exists("folder-entry-.config"))
        rule.onNode(hasContentDescription("repo (git リポジトリ)")).assertExists()
        rule.onNodeWithTag("folder-entry-repo").performClick()
        current("/home/u/repo")
        rule.onNodeWithTag("folder-entry-src").assertExists()

        rule.onNodeWithTag("folder-open").performClick()
        h.settle { h.core.callsOf<ApiCommand.OpenProject>().any { it.path == "/home/u/repo" } }
        h.settle { !h.exists("folder-browser") }
        h.settleFor()
        // 開いたらコーディングのビュー
        assertFalse(h.exists("dashboard"))
        assertTrue(h.sync.state.value.errors.isEmpty())
    }

    @Test
    fun `the parent entry and the path field move around the host`() {
        dashboard()
        openBrowser()
        current("/home/u")
        rule.onNodeWithTag("folder-parent").performClick()
        current("/home")
        rule.onNodeWithTag("folder-path").performTextClearance()
        rule.onNodeWithTag("folder-path").performTextInput("/tmp")
        rule.onNodeWithTag("folder-go").performClick()
        current("/tmp")
        rule.onNodeWithTag("folder-empty").assertExists()
    }

    @Test
    fun `a folder that cannot be listed is explained in place and the folder stays`() {
        dashboard()
        openBrowser()
        current("/home/u")
        rule.onNodeWithTag("folder-path").performTextClearance()
        rule.onNodeWithTag("folder-path").performTextInput("/nope")
        rule.onNodeWithTag("folder-go").performClick()
        h.settle { h.exists("folder-error") }
        rule.onNodeWithTag("folder-error").assertTextContains("does not exist", substring = true)
        rule.onNodeWithTag("folder-current", useUnmergedTree = true).assertTextEquals("/home/u")
    }

    @Test
    fun `a refused folder is explained in the browser, not in the banner`() {
        dashboard()
        openBrowser()
        current("/home/u")
        h.core.failWith(ApiCommand.OpenProject::class, ApiErrorCode.InvalidArgument, "not a git repository")
        rule.onNodeWithTag("folder-open").performClick()
        h.settle { h.exists("folder-error") }
        rule.onNodeWithTag("folder-error").assertTextContains("not a git repository", substring = true)
        assertTrue(h.exists("folder-browser"))
        assertTrue(h.sync.state.value.errors.isEmpty())
    }

    @Test
    fun `close leaves the browser without opening anything`() {
        dashboard()
        openBrowser()
        rule.onNodeWithTag("folder-close").performClick()
        h.settle { !h.exists("folder-browser") }
        assertEquals(listOf(PROJECT.path), h.core.callsOf<ApiCommand.OpenProject>().map { it.path })
        assertTrue(h.exists("dashboard"))
    }

    // ---- 利用統計 ----

    @Test
    fun `the usage statistics show the totals, the charts and the breakdowns`() {
        dashboard()
        h.settle { h.sync.state.value.stats.report != null }
        assertEquals(30L, h.core.callsOf<ApiCommand.GetUsageStats>().first().days)
        rule.onNodeWithTag("period-30").assertIsSelected()
        learnHostName()
        rule.onNodeWithTag("stats-host", useUnmergedTree = true).assertTextEquals("test-host")
        // 100+200+3000+400 + 200+400+6000+800 = 11,100
        rule.onNode(hasText("11.1K") and hasAnyAncestor(hasTestTag("stat-total")), useUnmergedTree = true).assertExists()
        rule.onNode(hasText("2") and hasAnyAncestor(hasTestTag("stat-sessions")), useUnmergedTree = true).assertExists()
        onStats("daily-chart").assertExists()
        onStats("heatmap").assertExists()
        rule.onNode(hasContentDescription("日別の利用ヒートマップ。最も多い日は 2026-10-20 (7.4K トークン)")).assertExists()
        rule.onNode(hasText("claude-opus-5-5"), useUnmergedTree = true).assertExists()
        rule.onNode(hasText("Claude Code") and hasAnyAncestor(hasTestTag("breakdown-0:claude-code")), useUnmergedTree = true).assertExists()
        rule.onNode(hasText("100%") and hasAnyAncestor(hasTestTag("breakdown-0:repo:repo")), useUnmergedTree = true).assertExists()
    }

    @Test
    fun `a tap on a day of the chart shows its numbers`() {
        dashboard()
        h.settle { h.sync.state.value.stats.report != null }
        onStats("daily-chart").performScrollTo().performTouchInput { click(Offset(width - 2f, height / 2f)) }
        rule.waitForIdle()
        onStats("bar-detail").assertTextContains("10/20: 7,400 トークン · 1 セッション", substring = true)
    }

    @Test
    fun `changing the period reads the statistics again`() {
        dashboard()
        h.settle { h.sync.state.value.stats.report != null }
        rule.onNodeWithTag("period-7").performScrollTo().performClick()
        h.settle { h.sync.state.value.stats.report?.periodDays == 7L }
        assertEquals(7L, h.core.callsOf<ApiCommand.GetUsageStats>().last().days)
        rule.onNodeWithTag("period-7").assertIsSelected()
        rule.onNodeWithTag("stats-refresh").performClick()
        h.settle { h.core.callsOf<ApiCommand.GetUsageStats>().last().refresh == true }
    }

    @Test
    fun `while the first read is on the section says it is loading`() {
        val gate = CompletableDeferred<Unit>()
        dashboard { usageStatsGate = gate }
        h.settle { h.sync.state.value.stats.loading }
        onStats("usage-loading").assertExists()
        gate.complete(Unit)
        h.settle { h.sync.state.value.stats.report != null }
        onStats("usage-loading").assertDoesNotExist()
        onStats("usage-report").assertExists()
    }

    @Test
    fun `a failed read shows the reason and retries`() {
        dashboard { failWith(ApiCommand.GetUsageStats::class, ApiErrorCode.Internal, "cannot read the logs") }
        h.settle { h.sync.state.value.stats.error != null }
        onStats("usage-error").assertExists()
        rule.onNode(hasText("cannot read the logs", substring = true), useUnmergedTree = true).assertExists()
        h.core.failures.remove(ApiCommand.GetUsageStats::class)
        rule.onNodeWithTag("stats-retry").performScrollTo().performClick()
        h.settle { h.sync.state.value.stats.report != null }
        onStats("usage-report").assertExists()
    }

    @Test
    fun `no logs on the host and no usage in the period have their own words`() {
        dashboard {
            usageStats = { days, tz ->
                cannedUsageStats(days, tz).copy(sources = listOf(StatsSource("claude-code", false, 0), StatsSource("codex", false, 0)))
            }
        }
        h.settle { h.sync.state.value.stats.report != null }
        onStats("usage-no-logs").assertExists()
        rule.onNode(hasText("Claude Code・Codex のセッションログがこのホストにありません", substring = true), useUnmergedTree = true).assertExists()

        val zero = StatsTokens(0, 0, 0, 0, 0)
        h.core.usageStats = { days, tz -> cannedUsageStats(days, tz).copy(totals = StatsTotals(zero, 0)) }
        rule.onNodeWithTag("period-7").performScrollTo().performClick()
        h.settle { h.sync.state.value.stats.report?.periodDays == 7L }
        onStats("usage-none").assertExists()
        rule.onNode(hasText("この 7 日間の利用はありません。"), useUnmergedTree = true).assertExists()
    }

    @Test
    fun `the credit links to agentsview`() {
        dashboard()
        val node = rule.onNodeWithTag("dashboard-credit", useUnmergedTree = true).performScrollTo().fetchSemanticsNode()
        val text = node.config[SemanticsProperties.Text].single()
        assertEquals("利用統計は agentsview (MIT) を参考にしています", text.text)
        val link = text.getLinkAnnotations(0, text.length).single().item
        assertEquals(AGENTSVIEW_URL, (link as androidx.compose.ui.text.LinkAnnotation.Url).url)
    }

    @Test
    fun `add project waits while the host is not connected`() {
        dashboard()
        h.core.state.value = io.github.tom039224.yhtye.shared.client.ConnectionState.Connecting(0)
        h.settle { h.sync.state.value.connection !is io.github.tom039224.yhtye.shared.client.ConnectionState.Connected }
        rule.onNodeWithTag("add-project").assertIsNotEnabled()
    }
}
