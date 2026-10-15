package io.github.tom039224.yhtye.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.performTextInput
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.BranchState
import io.github.tom039224.yhtye.shared.api.generated.BranchStatus
import io.github.tom039224.yhtye.shared.api.generated.BranchStatuses
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.ClientDevice
import io.github.tom039224.yhtye.shared.api.generated.ClientKind
import io.github.tom039224.yhtye.shared.api.generated.ClientServer
import io.github.tom039224.yhtye.shared.api.generated.Clients
import io.github.tom039224.yhtye.shared.api.generated.GitBranch
import io.github.tom039224.yhtye.shared.api.generated.GitWorktree
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.CLIENTS_SHOWN
import io.github.tom039224.yhtye.ui.panels.Dimmed
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
 * 左パネル: ビューの切り替え、プロジェクトのドロップダウン・バックグラウンドのチップ、ブランチとチャットの木、新しい
 * チャット・ブランチ、設定の入口。本物の同期層を偽のコアの上で動かし、操作がコアへの要求になることを確かめる。
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w400dp-h800dp")
class LeftPanelTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val h = MainHarness(rule)

    @After
    fun tearDown() = h.close()

    private val other = ProjectInfo(id = "other", name = "other-repo", path = "/tmp/other", open = false, running = false, awake = false)
    private val busy = ProjectInfo(id = "busy", name = "busy-repo", path = "/tmp/busy", open = true, running = true, awake = true)

    @Test
    fun `the picker lists the projects and a tap opens the chosen one`() {
        h.open { projects = listOf(PROJECT, other) }
        h.openLeft()
        rule.onNodeWithTag("project-name", useUnmergedTree = true).assertTextEquals(PROJECT.name)
        rule.onNodeWithTag("project-picker").performClick()
        rule.onNodeWithTag("project-option-${other.id}").performClick()
        // 一覧は閉じても、開く要求は最後まで進む (プロジェクトが替わる)
        h.settle { h.sync.state.value.project?.info?.id == other.id }
        assertFalse(h.exists("project-popup"))
    }

    @Test
    fun `the picker opens a repository by its path`() {
        h.open()
        h.openLeft()
        rule.onNodeWithTag("project-picker").performClick()
        rule.onNodeWithTag("open-path").performTextInput("/home/me/new-repo")
        rule.onNodeWithTag("open-submit").performClick()
        h.settle { h.core.callsOf<ApiCommand.OpenProject>().any { it.path == "/home/me/new-repo" } }
    }

    @Test
    fun `a tap outside the open list closes it`() {
        h.open()
        h.openLeft()
        rule.onNodeWithTag("project-picker").performClick()
        assertTrue(h.exists("project-popup"))
        rule.onNodeWithTag("project-popup-scrim").performClick()
        rule.waitForIdle()
        assertFalse(h.exists("project-popup"))
    }

    @Test
    fun `projects running in the background show as chips with the ring and a tap switches`() {
        h.open { projects = listOf(PROJECT, other, busy) }
        h.openLeft()
        // 開いていない other は出ない、表示中の repo も出ない
        assertFalse(h.exists("bg-project-${other.id}"))
        assertFalse(h.exists("bg-project-${PROJECT.id}"))
        rule.onNode(hasContentDescription("busy-repo に切り替え (実行中)")).assertExists()
        rule.onNodeWithTag("bg-project-${busy.id}").performClick()
        h.settle { h.sync.state.value.project?.info?.id == busy.id }
    }

    @Test
    fun `the picker lists the project it opened first and a long press moves one`() {
        h.open { projects = listOf(other, PROJECT) }
        h.openLeft()
        rule.onNodeWithTag("project-picker").performClick()
        val ids = { h.sync.state.value.projects.map { it.id } }
        // 開いたプロジェクトは使ったので先頭 (コアの一覧では other が先だった)
        assertEquals(listOf(PROJECT.id, other.id), ids())

        rule.onNodeWithTag("project-option-${other.id}").performTouchInput { longClick() }
        // 下の項目は「下へ」が選べない
        rule.onNodeWithTag("project-move-${other.id}-down").assertIsNotEnabled()
        rule.onNodeWithTag("project-move-${other.id}-up").performClick()
        h.settle { h.core.callsOf<ApiCommand.ReorderProjects>().isNotEmpty() }
        assertEquals(listOf(other.id, PROJECT.id), h.core.callsOf<ApiCommand.ReorderProjects>().single().ids)
        h.settle { ids() == listOf(other.id, PROJECT.id) }
        // 一覧は開いたまま
        assertTrue(h.exists("project-popup"))
    }

    @Test
    fun `the background chips trade the places they have among all the projects`() {
        val second = busy.copy(id = "busy2", name = "busy-two", path = "/tmp/busy2")
        h.open { projects = listOf(PROJECT, busy, second, other) }
        h.openLeft()
        rule.onNodeWithTag("bg-project-${busy.id}").performTouchInput { longClick() }
        rule.onNodeWithTag("bg-move-${busy.id}-up").assertIsNotEnabled()
        rule.onNodeWithTag("bg-move-${busy.id}-down").performClick()
        h.settle { h.core.callsOf<ApiCommand.ReorderProjects>().isNotEmpty() }
        // 開いていない other は動かない: busy と busy2 が入れ替わる
        assertEquals(listOf(PROJECT.id, second.id, busy.id, other.id), h.core.callsOf<ApiCommand.ReorderProjects>().single().ids)
    }

    @Test
    fun `worktrees follow the branch order and a long press moves one`() {
        val feat = "/tmp/feat-x"
        h.open {
            git = git.copy(
                branches = git.branches + GitBranch("feat/x", "f".repeat(40)),
                worktrees = git.worktrees + GitWorktree(feat, "feat/x", "f".repeat(40), isMain = false, missing = false),
            )
            branchOrder = listOf(feat)
        }
        h.openLeft()
        h.settle { h.sync.state.value.branchOrder?.keys == listOf(feat) }
        val order = { h.sync.state.value.branchOrder?.keys }
        // 作業ツリーの行は順どおり (feat/x が先)
        val rows = rule.onAllNodes(SemanticsMatcher("a worktree row") { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("worktree-") == true })
        rows[0].assert(hasTestTag("worktree-$feat"))

        rule.onNodeWithTag("worktree-$feat").performTouchInput { longClick() }
        rule.onNodeWithTag("worktree-move-$feat-up").assertIsNotEnabled()
        rule.onNodeWithTag("worktree-move-$feat-down").performClick()
        h.settle { h.core.callsOf<ApiCommand.ReorderBranches>().isNotEmpty() }
        assertEquals(listOf(h.core.worktree, feat), h.core.callsOf<ApiCommand.ReorderBranches>().single().keys)
        h.settle { order() == listOf(h.core.worktree, feat) }
    }

    @Test
    fun `other branches reorder from their long-press menu and follow another client`() {
        h.open {
            git = git.copy(branches = git.branches + listOf(GitBranch("topic", "t".repeat(40)), GitBranch("alpha", "a".repeat(40))))
        }
        h.openLeft()
        rule.onNodeWithTag("other-branches").performClick()
        rule.onNodeWithTag("other-branch-alpha").performTouchInput { longClick() }
        rule.onNodeWithTag("other-move-alpha-down").assertIsNotEnabled()
        rule.onNodeWithTag("other-move-alpha-up").performClick()
        h.settle { h.core.callsOf<ApiCommand.ReorderBranches>().isNotEmpty() }
        assertEquals(listOf("alpha", "topic"), h.core.callsOf<ApiCommand.ReorderBranches>().single().keys)
        // 他のクライアントの並べ替えも反映される
        h.core.emit(appWide(ApiEventBody.BranchOrderChanged(PROJECT.id, listOf("topic", "alpha"))))
        h.settle { h.sync.state.value.branchOrder?.keys == listOf("topic", "alpha") }
    }

    @Test
    fun `the project menu holds the move entries and sleep together`() {
        val awake = busy.copy(running = false)
        h.open { projects = listOf(PROJECT, awake) }
        h.openLeft()
        rule.onNodeWithTag("bg-project-${awake.id}").performTouchInput { longClick() }
        rule.onNodeWithTag("bg-move-${awake.id}-up").assertIsNotEnabled()
        rule.onNodeWithTag("bg-move-${awake.id}-down").assertIsNotEnabled()
        rule.onNodeWithTag("menu-sleep-${awake.id}").assertIsEnabled()
        // 一覧の行でも同じ: 並べ替えは一覧を開いたまま、スリープは閉じる
        rule.onNodeWithTag("project-picker").performClick()
        rule.onNodeWithTag("project-option-${awake.id}").performTouchInput { longClick() }
        rule.onNodeWithTag("project-move-${awake.id}-up").performClick()
        h.settle { h.core.callsOf<ApiCommand.ReorderProjects>().isNotEmpty() }
        assertTrue(h.exists("project-popup"))
    }

    @Test
    fun `a long press on a background chip opens a menu that sleeps the project`() {
        val awake = busy.copy(running = false)
        h.open { projects = listOf(PROJECT, awake) }
        h.openLeft()
        rule.onNodeWithTag("bg-project-${awake.id}").performTouchInput { longClick() }
        rule.onNodeWithTag("menu-sleep-${awake.id}").assertIsEnabled().performClick()
        h.settle { h.core.callsOf<ApiCommand.SleepProject>().isNotEmpty() }
        assertEquals(listOf(ApiCommand.SleepProject(awake.id)), h.core.callsOf<ApiCommand.SleepProject>())
        // 切り替えは起きない (メニューを開いただけ)
        assertEquals(listOf(PROJECT.path), h.core.callsOf<ApiCommand.OpenProject>().map { it.path })
        assertFalse(h.exists("menu-sleep-${awake.id}"))
    }

    @Test
    fun `sleep is disabled for a background project while a chat of it works`() {
        h.open { projects = listOf(PROJECT, busy) }
        h.openLeft()
        rule.onNodeWithTag("bg-project-${busy.id}").performTouchInput { longClick() }
        rule.onNodeWithTag("menu-sleep-${busy.id}").assertIsNotEnabled()
    }

    @Test
    fun `a background chip leaves once the core says its orchestrators stopped`() {
        val awake = busy.copy(running = false)
        h.open { projects = listOf(PROJECT, awake) }
        h.openLeft()
        assertTrue(h.exists("bg-project-${awake.id}"))
        rule.onNodeWithTag("bg-project-${awake.id}").performTouchInput { longClick() }
        rule.onNodeWithTag("menu-sleep-${awake.id}").performClick()
        h.settle { h.core.callsOf<ApiCommand.SleepProject>().isNotEmpty() }
        h.core.projects = listOf(PROJECT, awake.copy(awake = false))
        h.core.emit(ApiEvent(1, 0, awake.id, false, ApiEventBody.SessionStopped("orchestrator:C-1", suspended = false)))
        h.settle { !h.exists("bg-project-${awake.id}") }
    }

    @Test
    fun `a long press on a row of the picker sleeps that project and closes the list`() {
        val awake = busy.copy(running = false)
        h.open { projects = listOf(PROJECT, awake, other) }
        h.openLeft()
        rule.onNodeWithTag("project-picker").performClick()
        rule.onNodeWithTag("project-option-${other.id}").performTouchInput { longClick() }
        // 開いていないプロジェクトには眠らせるものが無い
        rule.onNodeWithTag("menu-sleep-${other.id}").assertIsNotEnabled()
        rule.onNodeWithTag("project-option-${awake.id}").performTouchInput { longClick() }
        rule.onNodeWithTag("menu-sleep-${awake.id}").assertIsEnabled().performClick()
        h.settle { h.core.callsOf<ApiCommand.SleepProject>().isNotEmpty() }
        assertEquals(listOf(ApiCommand.SleepProject(awake.id)), h.core.callsOf<ApiCommand.SleepProject>())
        assertEquals(listOf(PROJECT.path), h.core.callsOf<ApiCommand.OpenProject>().map { it.path })
        assertFalse(h.exists("project-popup"))
    }

    @Test
    fun `the project on screen sleeps from a long press on the picker button`() {
        h.open()
        h.openLeft()
        rule.onNodeWithTag("project-picker").performTouchInput { longClick() }
        // 開いたままのプロジェクトはオーケストレータが上がっているので眠らせられる
        rule.onNodeWithTag("menu-sleep-${PROJECT.id}").assertIsEnabled().performClick()
        h.settle { h.core.callsOf<ApiCommand.SleepProject>().isNotEmpty() }
        assertEquals(listOf(ApiCommand.SleepProject(PROJECT.id)), h.core.callsOf<ApiCommand.SleepProject>())
        assertFalse(h.exists("project-popup"))
    }

    @Test
    fun `sleep is disabled for the project on screen while its chats all sleep`() {
        val orchestrator = "orchestrator:C-1"
        val end = h.recording.end
        h.open(end.copy(sessions = end.sessions.filter { it.sessionKey != orchestrator }))
        h.openLeft()
        rule.onNodeWithTag("project-picker").performTouchInput { longClick() }
        rule.onNodeWithTag("menu-sleep-${PROJECT.id}").assertIsNotEnabled()
    }

    @Test
    fun `sleep is disabled for the project on screen while its orchestrator works`() {
        val orchestrator = "orchestrator:C-1"
        val end = h.recording.end
        h.open(end.copy(sessions = end.sessions.map { if (it.sessionKey == orchestrator) it.copy(turnRunning = true) else it }))
        h.openLeft()
        rule.onNodeWithTag("project-picker").performTouchInput { longClick() }
        rule.onNodeWithTag("menu-sleep-${PROJECT.id}").assertIsNotEnabled()
    }

    @Test
    fun `choosing a chat in the tree shows it in the center and closes the panel`() {
        val first = h.recording.end.chats.first()
        val second = ChatInfo("C-2", first.worktree, "Second chat", createdMs = 1, lastUsedMs = 1)
        h.open(h.recording.end.copy(chats = listOf(first, second)))
        h.openLeft()
        // メインクローンの印と、チャットの行
        rule.onNodeWithTag("main-clone", useUnmergedTree = true).assertExists()
        rule.onNodeWithTag("chat-C-2").performClick()
        h.settle { h.sync.state.value.project?.selectedChat == "C-2" }
        assertFalse(h.exists("left-panel"))
        rule.onNodeWithTag("chat-title").assertTextEquals("Second chat")
    }

    @Test
    fun `a worktree folds its chats`() {
        h.open()
        h.openLeft()
        val chat = h.recording.end.chats.first()
        assertTrue(h.exists("chat-${chat.id}"))
        rule.onNodeWithTag("worktree-${chat.worktree}").performClick()
        rule.waitForIdle()
        assertFalse(h.exists("chat-${chat.id}"))
    }

    @Test
    fun `a new chat in a worktree asks the core and returns to the center`() {
        h.open()
        h.openLeft()
        rule.onNodeWithTag("new-chat-${h.core.worktree}").performClick()
        h.settle { h.sync.state.value.project?.selectedChat == "C-9" }
        assertEquals(h.core.worktree, h.core.callsOf<ApiCommand.CreateChat>().single().worktree)
        assertFalse(h.exists("left-panel"))
    }

    @Test
    fun `the new branch form creates a branch from the chosen start point`() {
        h.open { git = git.copy(branches = git.branches + GitBranch("feat/x", "f".repeat(40))) }
        h.openLeft()
        rule.onNodeWithTag("new-branch").performClick()
        rule.onNodeWithTag("branch-name").performTextInput("feature/y")
        rule.onNodeWithTag("branch-from").performClick()
        rule.onNodeWithTag("branch-from-feat/x").performClick()
        rule.onNodeWithTag("branch-create").performClick()
        h.settle { h.sync.state.value.project?.selectedChat == "C-9" }
        val sent = h.core.callsOf<ApiCommand.CreateBranch>().single()
        assertEquals("feature/y", sent.name)
        assertEquals("feat/x", sent.from)
        assertFalse(h.exists("left-panel"))
    }

    @Test
    fun `a refused branch shows the core's reason in the form`() {
        h.open()
        h.core.fail(ApiCommand.CreateBranch::class, "a branch named 'main' already exists")
        h.openLeft()
        rule.onNodeWithTag("new-branch").performClick()
        rule.onNodeWithTag("branch-name").performTextInput("main")
        rule.onNodeWithTag("branch-create").performClick()
        h.settle { h.exists("branch-error") }
        rule.onNodeWithTag("branch-error").assertTextEquals("a branch named 'main' already exists")
        assertTrue(h.exists("left-panel"))
    }

    @Test
    fun `other branches fold and start a chat there`() {
        h.open { git = git.copy(branches = git.branches + GitBranch("feat/x", "f".repeat(40))) }
        h.openLeft()
        assertFalse(h.exists("other-branch-feat/x"))
        rule.onNodeWithTag("other-branches").performClick()
        rule.onNodeWithTag("other-branch-feat/x").performClick()
        h.settle { h.core.callsOf<ApiCommand.CreateChat>().isNotEmpty() }
        assertEquals("feat/x", h.core.callsOf<ApiCommand.CreateChat>().single().branch)
    }

    @Test
    fun `branch rows show the PR merge status as an icon with its Japanese label`() {
        h.open {
            git = git.copy(
                branches = git.branches + listOf("feat/x", "ship", "redo", "old").map { GitBranch(it, it.first().toString().repeat(40)) },
                worktrees = git.worktrees + listOf("feat/x", "ship", "redo").map {
                    GitWorktree("/tmp/$it", it, it.first().toString().repeat(40), isMain = false, missing = false)
                },
            )
            branchStatuses = BranchStatuses(
                "main",
                listOf(
                    BranchStatus("feat/x", BranchState.PrOpen, 7, "https://example.test/u/repo/pull/7"),
                    BranchStatus("ship", BranchState.Merged, 9, "https://example.test/u/repo/pull/9"),
                    BranchStatus("redo", BranchState.MergedWithChanges, 8, "https://example.test/u/repo/pull/8"),
                    BranchStatus("old", BranchState.Unmerged, null, null),
                ),
            )
        }
        h.openLeft()
        h.settle { h.sync.state.value.git?.statuses != null }
        // 既定のブランチはラベルの無いブランチのアイコンのまま
        rule.onNodeWithTag("main-clone", useUnmergedTree = true)
            .assertExists()
            .assert(SemanticsMatcher.keyNotDefined(SemanticsProperties.ContentDescription))
        // 開いた PR は PR のアイコン、マージ済みはマージのアイコン (その後の変更があれば点も)
        assertStatusIcon("PR 作成済み (#7)", "pullRequest")
        assertStatusIcon("PR でマージ済み (#9)", "merge")
        assertStatusIcon("PR でマージ済み・未マージの変更あり (#8)", "merge")
        rule.onAllNodesWithTag("branch-status-badge", useUnmergedTree = true).assertCountEquals(1)
        rule.onNode(
            hasTestTag("branch-status-badge") and hasAnyAncestor(hasContentDescription("PR でマージ済み・未マージの変更あり (#8)")),
            useUnmergedTree = true,
        ).assertExists()
        // 作業ツリーの無いブランチにもアイコンが出る
        rule.onNodeWithTag("other-branches").performClick()
        assertStatusIcon("まだ取り込まれていません", "branch")
    }

    /** [label] の付いた状態のアイコンが [icon] (YIcons の名前) の形で出ている。 */
    private fun assertStatusIcon(label: String, icon: String) {
        rule.onNode(hasTestTag("branch-icon-$icon") and hasAnyAncestor(hasContentDescription(label)), useUnmergedTree = true).assertExists()
    }

    @Test
    fun `a branch without an entry reads unknown and the statuses are reused`() {
        h.open {
            git = git.copy(
                worktrees = git.worktrees + GitWorktree("/tmp/mystery", "mystery", "m".repeat(40), isMain = false, missing = false),
            )
        }
        h.openLeft()
        h.settle { h.sync.state.value.git?.statuses != null }
        assertStatusIcon("状態不明", "help")
        assertEquals(1, h.core.callsOf<ApiCommand.GetBranchStatuses>().size)
    }

    @Test
    fun `the rail switches to the history view, which has no right panel`() {
        h.open()
        h.openLeft()
        rule.onNodeWithTag("rail-runs").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("runs-empty").assertExists()
        // 左の中身と中央の両方が PC の RunsView と同じ空の状態
        rule.onAllNodesWithText("現状は何もありません", useUnmergedTree = true).assertCountEquals(2)
        assertFalse(h.exists("project-picker"))
        rule.onNodeWithTag("panel-shield").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("runs-center").assertExists()
        // 右パネルへのスワイプは動かない
        h.swipeScreen(0.8f, 0.1f)
        assertFalse(h.exists("panel-right"))

        h.openLeft()
        rule.onNodeWithTag("rail-work").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("project-picker").assertExists()
    }

    @Test
    fun `settings opens from the rail and the back button closes it`() {
        h.open()
        h.openLeft()
        rule.onNodeWithTag("rail-settings").performClick()
        h.settle { h.exists("settings") }
        rule.onNodeWithTag("settings-back").performClick()
        h.settle { !h.exists("settings") }
    }

    @Test
    fun `the footer shows the host and a tap on the usage asks the harness again`() {
        h.open()
        h.openLeft()
        rule.onNodeWithTag("host").assertExists()
        rule.onNodeWithTag("usage-5h", useUnmergedTree = true).assertExists()
        rule.onNodeWithTag("usage-week", useUnmergedTree = true).assertExists()
        h.core.calls.clear()
        rule.onNodeWithTag("usage").performClick()
        h.settle { h.core.callsOf<ApiCommand.GetUsage>().any { it.refresh == true } }
        val dot = rule.onNodeWithTag("host-dot").fetchSemanticsNode().config[SemanticsProperties.ContentDescription]
        assertEquals(listOf("接続を確認中"), dot) // ping は前面にある間だけ: まだ応答が無い
    }

    // ---- サーバーとクライアントの一覧 (T-74) ----

    private val pixel = ClientDevice("dev-1", "Pixel 9", ClientKind.Android, connected = true, lastSeenMs = 2_000)
    private val tablet = ClientDevice("dev-2", "Tablet", ClientKind.Android, connected = false, lastSeenMs = 1_000)

    /** ハーネスの端末 ID ([MainHarness] が `MainScreen` に渡す)。 */
    private val self = ClientDevice("device", "My Pixel 8a", ClientKind.Android, connected = true, lastSeenMs = 3_000)

    private fun iconIn(row: String, icon: String) =
        rule.onAllNodes(hasTestTag(icon) and hasAnyAncestor(hasTestTag(row)), useUnmergedTree = true)

    private fun rowTags(): List<String?> = rule.onAllNodes(hasAnyAncestor(hasTestTag("client-list")) and hasTestTagPrefix("client-dev"))
        .fetchSemanticsNodes().map { it.config.getOrNull(SemanticsProperties.TestTag) }

    private fun dimmed(tag: String, value: Boolean) = rule.onNodeWithTag(tag).assert(SemanticsMatcher.expectValue(Dimmed, value))

    @Test
    fun `the server row has the server icon and there are no device rows without devices`() {
        h.open()
        h.openLeft()
        iconIn("host", "server-icon").assertCountEquals(1)
        dimmed("host", false)
        assertFalse(h.exists("client-list"))
        assertTrue(h.core.callsOf<ApiCommand.ListClients>().isNotEmpty())
    }

    @Test
    fun `the paired devices have the phone icon, this device is marked and offline ones are greyed out`() {
        h.open { devices = listOf(tablet, self, pixel) }
        h.openLeft()
        // 接続中が先 (最終接続の新しい順)
        assertEquals(listOf("client-device", "client-dev-1", "client-dev-2"), rowTags())
        listOf("client-device", "client-dev-1", "client-dev-2").forEach { iconIn(it, "phone-icon").assertCountEquals(1) }
        iconIn("client-dev-1", "server-icon").assertCountEquals(0)
        dimmed("client-device", false)
        dimmed("client-dev-1", false)
        dimmed("client-dev-2", true)
        rule.onAllNodes(hasTestTag("client-self"), useUnmergedTree = true).assertCountEquals(1)
        iconIn("client-device", "client-self").assertCountEquals(1)
        rule.onNodeWithTag("client-device").assert(hasContentDescription("My Pixel 8a (この端末) · Android · 接続中"))
        rule.onNodeWithTag("client-dev-2").assert(hasContentDescription("Tablet · Android · オフライン · 最終接続", substring = true))
    }

    @Test
    fun `without the connection to the PC the server row and this device are greyed out`() {
        h.open { devices = listOf(self, pixel) }
        h.openLeft()
        h.core.state.value = ConnectionState.WaitingToReconnect(1, 60_000, "gone")
        h.settle { h.sync.state.value.connection !is ConnectionState.Connected }
        dimmed("host", true)
        dimmed("client-device", true)
        // 他の端末は最後に読んだまま
        dimmed("client-dev-1", false)
    }

    @Test
    fun `the rows follow clients_changed`() {
        h.open { devices = listOf(pixel, tablet) }
        h.openLeft()
        h.core.emit(appWide(ApiEventBody.ClientsChanged(Clients(ClientServer("test-host"), listOf(pixel, tablet.copy(connected = true))))))
        h.settle { h.sync.state.value.clients.all { it.connected } }
        dimmed("client-dev-2", false)
        h.core.emit(appWide(ApiEventBody.ClientsChanged(Clients(ClientServer("test-host"), emptyList()))))
        h.settle { h.sync.state.value.clients.isEmpty() }
        assertFalse(h.exists("client-list"))
    }

    @Test
    fun `devices past the first three fold into 他 N 台 and unfold`() {
        val many = (0 until 7).map { tablet.copy(id = "dev-x$it", name = "Phone $it", lastSeenMs = 1_000L - it) }
        h.open { devices = many }
        h.openLeft()
        assertEquals(CLIENTS_SHOWN, rowTags().size)
        rule.onNodeWithTag("client-more").assert(hasText("他 4 台", substring = true)).performClick()
        rule.waitForIdle()
        assertEquals(7, rowTags().size)
        rule.onNodeWithTag("client-more").assert(hasText("折りたたむ", substring = true)).performClick()
        rule.waitForIdle()
        assertEquals(CLIENTS_SHOWN, rowTags().size)
    }

    @Test
    fun `exactly three devices are not folded`() {
        h.open { devices = listOf(self, pixel, tablet) }
        h.openLeft()
        assertEquals(3, rowTags().size)
        assertFalse(h.exists("client-more"))
    }
}

private fun hasTestTagPrefix(prefix: String) =
    SemanticsMatcher("TestTag starts with $prefix") { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith(prefix) == true }
