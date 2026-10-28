package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.GitBranch
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** PC の `src/store/order.test.ts` と、`multiClient.test.ts` の順序の部分の移植: 並べ替えの関数と、コアのリストとの同期。 */
@OptIn(ExperimentalCoroutinesApi::class)
class OrderTest {
    private val core = FakeCore(PROJECT, chatSnapshot()).apply { log = CHAT_LOG }
    private val other = ProjectInfo("other", "other", "/tmp/other", open = false, running = false, awake = false)

    // ---- 関数 ----

    @Test
    fun sortsListedItemsFirstAndTheRestAfterInTheirOwnOrder() {
        val items = listOf("n1", "b", "n2", "a")
        assertEquals(listOf("a", "b", "n1", "n2"), sortByOrder(items, listOf("a", "gone", "b")) { it })
        assertEquals(items, sortByOrder(items, emptyList()) { it })
    }

    @Test
    fun stepsAKeyByOneAndRefusesPastTheEnds() {
        assertEquals(listOf("b", "a", "c"), step(listOf("a", "b", "c"), "b", -1))
        assertEquals(listOf("a", "c", "b"), step(listOf("a", "b", "c"), "b", 1))
        assertNull(step(listOf("a", "b", "c"), "a", -1))
        assertNull(step(listOf("a", "b", "c"), "c", 1))
        assertNull(step(listOf("a"), "zz", 1))
    }

    @Test
    fun putsAShownSubsetBackIntoThePlacesItHad() {
        assertEquals(listOf("c", "x", "a", "y", "b"), fillSlots(listOf("a", "x", "b", "y", "c"), listOf("c", "a", "b")))
        assertEquals(listOf("a", "b"), fillSlots(listOf("a", "b"), emptyList()))
    }

    @Test
    fun theTreeFollowsTheBranchOrder() {
        val overview = CHAT_GIT.copy(branches = CHAT_GIT.branches + listOf(GitBranch("topic", "t".repeat(40)), GitBranch("alpha", "a".repeat(40))))
        val natural = buildTree(CHAT_INFOS, overview)
        assertEquals(listOf(MAIN_WT, FEAT_WT), natural.worktrees.map { it.path })
        assertEquals(listOf("topic", "alpha"), natural.otherBranches)

        val ordered = buildTree(CHAT_INFOS, overview, order = listOf(FEAT_WT, "alpha", "gone"))
        assertEquals(listOf(FEAT_WT, MAIN_WT), ordered.worktrees.map { it.path })
        assertEquals(listOf("alpha", "topic"), ordered.otherBranches)
        // ノードの中のチャットはそれぞれの順 (新しい順) のまま
        assertEquals(listOf("C-2", "C-1"), ordered.worktrees.flatMap { w -> w.chats.map { it.id } })
    }

    // ---- コアのリストとの同期 ----

    @Test
    fun readsTheBranchOrderOfTheProjectOnScreenAndReplacesItWithEachEvent() = runTest {
        core.branchOrder = listOf(MAIN_WT)
        val s = newStore(core, MemoryPrefs())
        openAndSettle(s)
        assertEquals(BranchOrderView(PROJECT.id, listOf(MAIN_WT)), s.state.value.branchOrder)
        push(core, appWide(ApiEventBody.BranchOrderChanged(PROJECT.id, listOf(FEAT_WT, MAIN_WT))))
        assertEquals(listOf(FEAT_WT, MAIN_WT), s.state.value.branchOrder?.keys)
        // 別のプロジェクトの順は、いま表示しているものではない
        push(core, appWide(ApiEventBody.BranchOrderChanged("other", listOf("z"))))
        assertEquals(listOf(FEAT_WT, MAIN_WT), s.state.value.branchOrder?.keys)
        // 開いているプロジェクトには畳み込まない
        assertEquals(CHAT_LOG.size.toLong(), s.view().cursor)
    }

    @Test
    fun readsTheBranchOrderAgainAfterAResync() = runTest {
        val s = newStore(core, MemoryPrefs())
        openAndSettle(s)
        core.branchOrder = listOf("a", "b")
        push(core, appWide(ApiEventBody.Resync))
        assertEquals(listOf("a", "b"), s.state.value.branchOrder?.keys)
    }

    @Test
    fun showsABranchReorderAtOnceAndKeepsTheCoresAnswer() = runTest {
        core.branchOrder = listOf("hidden")
        val s = newStore(core, MemoryPrefs())
        openAndSettle(s)
        val job = backgroundScope.launch { s.reorderBranches(listOf(MAIN_WT, FEAT_WT)) }
        runCurrent()
        job.join()
        assertEquals(listOf(MAIN_WT, FEAT_WT), core.callsOf<ApiCommand.ReorderBranches>().last().keys)
        assertEquals(listOf(MAIN_WT, FEAT_WT, "hidden"), s.state.value.branchOrder?.keys)
    }

    @Test
    fun reordersTheProjectsAndPutsAnOpenedProjectFirst() = runTest {
        core.others = listOf(other)
        val s = newStore(core, MemoryPrefs())
        openAndSettle(s)
        s.refreshProjects()
        assertEquals(listOf(PROJECT.id, "other"), s.state.value.projects.map { it.id })

        s.reorderProjects(listOf("other", PROJECT.id))
        assertEquals(listOf("other", PROJECT.id), core.callsOf<ApiCommand.ReorderProjects>().last().ids)
        assertEquals(listOf("other", PROJECT.id), s.state.value.projects.map { it.id })

        s.openProject(PROJECT.path)
        runCurrent()
        assertEquals(PROJECT.id, s.state.value.projects.first().id)
    }
}
