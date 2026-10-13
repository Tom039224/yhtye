package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.BranchState
import io.github.tom039224.yhtye.shared.api.generated.BranchStatus
import io.github.tom039224.yhtye.shared.api.generated.BranchStatuses
import io.github.tom039224.yhtye.shared.api.generated.GitBranch
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.SessionRecord
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** PC の `src/ui/branchTree.test.ts` の移植。 */
class BranchTreeTest {
    private fun session(chat: String, live: Boolean, turn: Boolean): SessionRecord =
        sessionRecord(orchestratorKey(chat), if (live) SessionStatus.Live else SessionStatus.Stopped, turn)

    private fun viewWith(sessions: List<SessionRecord> = emptyList()): ProjectView =
        applySnapshot(newProjectView(PROJECT), chatSnapshot()).copy(sessions = sessions)

    // ---- buildTree ----

    @Test
    fun makesANodePerWorktreeNamedByItsBranchWithItsChatsNewestFirst() {
        val tree = buildTree(CHAT_INFOS, CHAT_GIT)
        assertEquals(
            listOf(Triple("main", true, listOf("C-1")), Triple("feat/x", false, listOf("C-2"))),
            tree.worktrees.map { Triple(it.label, it.isMain, it.chats.map { c -> c.id }) },
        )
        assertEquals(emptyList(), tree.lost)
        assertEquals(emptyList(), tree.otherBranches)
    }

    @Test
    fun hidesYhtyesInternalWorktreesAndBranchesButListsTheBranchesNothingHasCheckedOut() {
        val overview = CHAT_GIT.copy(branches = CHAT_GIT.branches + GitBranch("topic", "t".repeat(40)))
        val tree = buildTree(CHAT_INFOS, overview)
        assertEquals(listOf(MAIN_WT, FEAT_WT), tree.worktrees.map { it.path })
        assertEquals(listOf("topic"), tree.otherBranches)
    }

    @Test
    fun putsTheChatsOfAWorktreeGitNoLongerListsUnderLost() {
        val gone = ChatInfo("C-3", "/gone", null, 1, 1)
        val tree = buildTree(CHAT_INFOS + gone, CHAT_GIT)
        assertEquals(listOf(gone), tree.lost)
        assertTrue(tree.worktrees.flatMap { w -> w.chats.map { it.id } }.none { it == "C-3" })
    }

    @Test
    fun namesWorktreesByTheirDirectoryUntilGitHasBeenRead() {
        val tree = buildTree(CHAT_INFOS, null)
        assertEquals(listOf("feat-x", "repo"), tree.worktrees.map { it.label })
        assertEquals(Place.Unknown, placeOf(CHAT_INFOS[0], null))
    }

    @Test
    fun marksAWorktreeWhoseDirectoryIsGoneAndKeepsItsChats() {
        val overview = CHAT_GIT.copy(worktrees = CHAT_GIT.worktrees.map { if (it.path == FEAT_WT) it.copy(missing = true) else it })
        val node = buildTree(CHAT_INFOS, overview).worktrees.first { it.path == FEAT_WT }
        assertTrue(node.missing)
        assertEquals(listOf("C-2"), node.chats.map { it.id })
    }

    @Test
    fun showsAChatMadeAfterGitWasReadUnderItsDirectoryNotAsLost() {
        // multiClient.test.ts の placeOf の確かめを木にも (Kotlin で足した)
        val made = ChatInfo("C-3", "/data/worktrees/repo/branches/new", null, 1, 1)
        val tree = buildTree(CHAT_INFOS + made, CHAT_GIT, awaiting = listOf("C-3"))
        assertEquals(emptyList(), tree.lost)
        assertEquals("new", tree.worktrees.last().label)
        assertEquals(Place.Unknown, placeOf(made, CHAT_GIT, awaiting = true))
        assertEquals(Place.Missing, placeOf(made, CHAT_GIT, awaiting = false))
    }

    // ---- chatRing ----

    @Test
    fun isStoppedWithoutASessionReadyWithAnIdleOneAndRunningDuringATurn() {
        assertEquals(Ring.Stopped, chatRing(viewWith(), "C-1"))
        assertEquals(Ring.Stopped, chatRing(viewWith(listOf(session("C-1", false, false))), "C-1"))
        assertEquals(Ring.Ready, chatRing(viewWith(listOf(session("C-1", true, false))), "C-1"))
        assertEquals(Ring.Running, chatRing(viewWith(listOf(session("C-1", true, true))), "C-1"))
    }

    // ---- shortAge ----

    private val now = 10L * 24 * 3_600_000

    @Test
    fun saysNowUnderAMinuteThenMinutesHoursAndDays() {
        assertEquals("now", shortAge(now, now - 59_000))
        assertEquals("3m", shortAge(now, now - 3 * 60_000))
        assertEquals("59m", shortAge(now, now - 59 * 60_000))
        assertEquals("2h", shortAge(now, now - 2 * 3_600_000))
        assertEquals("3d", shortAge(now, now - 3 * 24 * 3_600_000))
    }

    @Test
    fun saysNowForATimeInTheFuture() {
        assertEquals("now", shortAge(now, now + 60_000))
    }

    // ---- deleteBlocker ----

    @Test
    fun allowsDeletingAChatWhoseGroupsAreAllFinishedAndWhoseOrchestratorIsIdle() {
        // C-1 のグループ (G-1) は完了
        assertNull(deleteBlocker(viewWith(), "C-1"))
        assertNull(deleteBlocker(viewWith(listOf(session("C-1", true, false))), "C-1"))
    }

    @Test
    fun namesTheUnfinishedGroupThatKeepsAChat() {
        assertTrue(deleteBlocker(viewWith(), "C-2").orEmpty().contains("G-2"))
    }

    @Test
    fun refusesWhileTheOrchestratorIsWorking() {
        assertTrue(deleteBlocker(viewWith(listOf(session("C-1", true, true))), "C-1").orEmpty().contains("実行中"))
    }

    @Test
    fun countsAGroupWhoseMergeIsBlockedAsUnfinished() {
        val v = viewWith()
        val state = v.state!!.let { s ->
            s.copy(groups = s.groups.map { if (it.id == "G-1") it.copy(status = GroupStatus.MergeBlocked) else it })
        }
        assertTrue(deleteBlocker(v.copy(state = state), "C-1").orEmpty().contains("G-1"))
    }

    // ---- statusLabel / statusOf ----

    @Test
    fun labelsEveryBranchStateInJapanese() {
        assertEquals("まだ取り込まれていません", statusLabel(BranchState.Unmerged, null))
        assertEquals("PR 作成済み (#7)", statusLabel(BranchState.PrOpen, 7))
        assertEquals("PR でマージ済み (#9)", statusLabel(BranchState.Merged, 9))
        assertEquals("取り込み済み", statusLabel(BranchState.Merged, null))
        assertEquals("PR でマージ済み・未マージの変更あり (#8)", statusLabel(BranchState.MergedWithChanges, 8))
        assertEquals("状態不明", statusLabel(BranchState.Unknown, null))
        assertEquals("状態不明", statusLabel(BranchState.Unrecognized, null))
    }

    @Test
    fun statusOfIsNullForTheDefaultBranchAndUnknownWithoutAnEntry() {
        val statuses = BranchStatuses(
            "main",
            listOf(BranchStatus("feat/x", BranchState.PrOpen, 7, "https://example.test/pull/7")),
        )
        assertNull(statusOf("main", statuses))
        assertNull(statusOf(null, statuses))
        assertEquals(BranchState.PrOpen to 7L, statusOf("feat/x", statuses))
        assertEquals(BranchState.Unknown to null, statusOf("other", statuses))
        assertEquals(BranchState.Unknown to null, statusOf("feat/x", null))
    }
}
