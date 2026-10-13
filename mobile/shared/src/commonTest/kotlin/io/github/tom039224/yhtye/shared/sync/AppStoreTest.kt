package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.client.ConnectionState
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

/** PC の `src/store/app.test.ts` の移植 (同期・遅延の履歴・バックグラウンドの動作中)。 */
@OptIn(ExperimentalCoroutinesApi::class)
class AppStoreTest {
    private val log = durable(FULL_RUN)
    private val last = log.last().seq
    private val core = FakeCore(PROJECT, FULL_RUN.start)

    /** 会話の行の元の durable の seq (重なりを見つける)。 */
    private fun AppStore.itemSeqs(): List<Long> = view().transcripts.values.flatten().map { it.seq }

    private fun AppStore.assertNoDuplicates() {
        val seqs = itemSeqs()
        assertEquals(seqs.size, seqs.toSet().size, "duplicated items: $seqs")
    }

    // ---- 同期 ----

    @Test
    fun loadsSnapshotAndHistoryInPagesAndEndsAtTheCoresState() = runTest {
        core.log = log
        core.pageCap = 7
        val store = newStore(core)
        openAndSettle(store)
        assertEquals(last, store.view().cursor)
        assertEquals(FULL_RUN.end.state, store.view().state)
        assertEquals(FULL_RUN.end.sessions, store.view().sessions)
        assertEquals(listOf(0L, 7, 14, 21, 28, 35, 42, 49), core.callsOf<ApiCommand.ListEvents>().map { it.afterSeq })
    }

    @Test
    fun buffersEventsPushedDuringTheLoadAndAppliesEachOnce() = runTest {
        val half = log.filter { it.seq <= 30 }
        core.log = half
        val next = log.first { it.seq == 31L }
        val store = newStore(core)
        core.before = { cmd ->
            if (cmd is ApiCommand.GetSnapshot) {
                // 読み込み中に押し込まれる: 保存済みの重複と新しいもの
                FULL_RUN.events.filter { it.seq in 28..30 }.forEach(core::emit)
                core.log = half + next
                core.emit(next)
            }
        }
        openAndSettle(store)
        assertEquals(31, store.view().cursor)
        store.assertNoDuplicates()
    }

    @Test
    fun catchesUpWithListEventsWhenADurableEventIsMissing() = runTest {
        core.log = log.filter { it.seq <= 20 }
        val store = newStore(core)
        openAndSettle(store)
        core.log = log
        push(core, log.last()) // 21..LAST-1 は届かなかった
        assertEquals(last, store.view().cursor)
        assertEquals(20, core.callsOf<ApiCommand.ListEvents>().last().afterSeq)
        assertEquals(FULL_RUN.end.state, store.view().state)
        // 遅れて届いた重複は何も変えない
        val before = store.view()
        push(core, log[10])
        assertSame(before, store.view())
    }

    @Test
    fun treatsALiveEventFromAfterAnUnseenDurableEventAsAGap() = runTest {
        core.log = log.filter { it.seq <= 5 }
        val store = newStore(core)
        openAndSettle(store)
        core.log = log.filter { it.seq <= 7 }
        push(core, chunk(7, ORCHESTRATOR, "late"))
        assertEquals(7, store.view().cursor)
    }

    @Test
    fun streamsTextAndDropsStaleChunksReplayedAfterACatchUp() = runTest {
        core.log = emptyList()
        core.snapshot = FULL_RUN.start.copy(seq = 0, sessions = emptyList())
        val store = newStore(core)
        openAndSettle(store)
        push(core, chunk(0, ORCHESTRATOR, "Hi "), chunk(0, ORCHESTRATOR, "there"))
        assertEquals("Hi there", store.view().streaming[ORCHESTRATOR]?.message)
        push(core, agentText(1, ORCHESTRATOR, "Hi there"))
        assertEquals("", store.view().streaming[ORCHESTRATOR]?.message)
        push(core, chunk(0, ORCHESTRATOR, "stale"))
        assertEquals("", store.view().streaming[ORCHESTRATOR]?.message)
    }

    @Test
    fun reopensAndCatchesUpAfterAReconnectDroppingPartialStreams() = runTest {
        core.log = log.filter { it.seq <= 10 }
        val store = newStore(core)
        openAndSettle(store)
        push(core, chunk(10, ORCHESTRATOR, "partial"))
        assertEquals("partial", store.view().streaming[ORCHESTRATOR]?.message)
        core.state.value = ConnectionState.WaitingToReconnect(1, 1_000, "fell behind; reconnect")
        runCurrent()
        assertEquals(ConnectionState.WaitingToReconnect(1, 1_000, "fell behind; reconnect"), store.state.value.connection)
        assertNull(store.view().streaming[ORCHESTRATOR])
        core.log = log
        val opens = core.callsOf<ApiCommand.OpenProject>().size
        core.state.value = ConnectionState.Connected(2)
        runCurrent()
        assertEquals(last, store.view().cursor)
        assertEquals(opens + 1, core.callsOf<ApiCommand.OpenProject>().size)
        assertEquals(10, core.callsOf<ApiCommand.ListEvents>().last().afterSeq)
        store.assertNoDuplicates()
    }

    @Test
    fun treatsANewSessionNumberAsAReconnectEvenWithoutSeeingTheDisconnect() = runTest {
        // Kotlin で足した: StateFlow は途中の状態を飛ばし得る (Connected(1) → Connected(2))
        core.log = log.filter { it.seq <= 10 }
        val store = newStore(core)
        openAndSettle(store)
        push(core, chunk(10, ORCHESTRATOR, "partial"))
        core.log = log
        core.state.value = ConnectionState.Connected(2)
        runCurrent()
        assertEquals(last, store.view().cursor)
        assertNull(store.view().streaming[ORCHESTRATOR])
    }

    @Test
    fun showsLoadFailuresAndCommandErrors() = runTest {
        core.fail(ApiCommand.GetSnapshot::class, ApiErrorCode.NotFound, "project repo is not open")
        val store = newStore(core)
        store.openProject(PROJECT.path)
        runCurrent()
        assertEquals(LoadPhase.Error, store.view().phase)
        assertTrue(store.state.value.errors.last().message.contains("project repo is not open"))

        core.failures.clear()
        core.log = log
        openAndSettle(store)
        core.fail(ApiCommand.SendUserMessage::class, ApiErrorCode.Unavailable, "the project's orchestration has stopped")
        assertFalse(store.sendMessage("hello"))
        assertTrue(store.state.value.errors.last().message.contains("orchestration has stopped"))
        assertTrue(store.state.value.errors.last().message.startsWith("send_user_message failed: "))
        val count = store.state.value.errors.size
        store.dismissError(store.state.value.errors[0].id)
        runCurrent()
        assertEquals(count - 1, store.state.value.errors.size)
    }

    @Test
    fun sendsUserCommandsForTheOpenProject() = runTest {
        core.log = log
        val store = newStore(core)
        openAndSettle(store)
        assertTrue(store.sendMessage("do it"))
        store.cancelTurn()
        store.cancelTask("T-1")
        store.cancelGroup("G-1")
        store.retryGroupMerge("G-2")
        store.compactChat()
        assertEquals(
            listOf(
                ApiCommand.SendUserMessage("repo", "C-1", "do it"),
                ApiCommand.CancelOrchestratorTurn("repo", "C-1"),
                ApiCommand.CancelTask("repo", "T-1"),
                ApiCommand.CancelGroup("repo", "G-1"),
                ApiCommand.RetryGroupMerge("repo", "G-2"),
                ApiCommand.CompactChat("repo", "C-1"),
            ),
            core.calls.takeLast(6),
        )
    }

    @Test
    fun ignoresEventsOfOtherProjectsAndTracksTheOrchestratorsTurn() = runTest {
        core.log = log
        val store = newStore(core)
        openAndSettle(store)
        val before = store.view()
        push(core, prompted(last + 1, ORCHESTRATOR).copy(project = "other"))
        assertSame(before, store.view())
        push(core, prompted(last + 1, ORCHESTRATOR))
        assertEquals(true, store.view().sessions.find { it.sessionKey == ORCHESTRATOR }?.turnRunning)
    }

    @Test
    fun reopensTheProjectOpenedLastWhenItFirstConnects() = runTest {
        // PC の reopenLast (ページの読み込み後) と同じ: 最初につながったら覚えていたプロジェクトを開く
        core.log = log
        val prefs = MemoryPrefs(mapOf(LAST_PROJECT_KEY to PROJECT.path))
        val store = newStore(core, prefs)
        assertEquals(LoadPhase.Ready, store.view().phase)
        assertTrue(store.state.value.projectsLoaded)
    }

    // ---- 遅延の履歴 ----

    private suspend fun TestScope.openWindowed(): AppStore {
        val store = newStore(core, options = AppStore.Options(historyWindow = 10, historyPage = 7))
        core.log = log
        core.snapshot = FULL_RUN.end
        openAndSettle(store)
        assertEquals(last - 9, store.view().historyStart)
        return store
    }

    private fun olderCalls() = core.callsOf<ApiCommand.ListEvents>().filter { it.afterSeq < last - 10 }

    @Test
    fun readsOnePageForOverlappingRequests() = runTest {
        val store = openWindowed()
        List(3) { async { store.loadOlderHistory() } }.awaitAll()
        assertEquals(last - 16, store.view().historyStart)
        assertEquals(1, olderCalls().size)
        assertFalse(store.view().loadingOlder)
    }

    @Test
    fun doesNothingOnceTheWholeLogIsLoaded() = runTest {
        val store = openWindowed()
        while (store.view().historyStart > 1) store.loadOlderHistory()
        val calls = core.callsOf<ApiCommand.ListEvents>().size
        store.loadOlderHistory()
        assertEquals(calls, core.callsOf<ApiCommand.ListEvents>().size)
        // 全部読んだ会話は最初から読んだものと同じ
        val whole = log.fold(applySnapshot(newProjectView(PROJECT), FULL_RUN.end), ::applyDurable)
        assertEquals(whole.transcripts, store.view().transcripts)
    }

    @Test
    fun keepsAFailureForTheHistoryRowAndARetryClearsIt() = runTest {
        val store = openWindowed()
        core.fail(ApiCommand.ListEvents::class, ApiErrorCode.Unavailable, "bridge restarted")
        store.loadOlderHistory()
        assertEquals("bridge restarted", store.view().olderError)
        assertFalse(store.view().loadingOlder)
        assertEquals(last - 9, store.view().historyStart)

        core.failures.remove(ApiCommand.ListEvents::class)
        store.loadOlderHistory()
        assertNull(store.view().olderError)
        assertEquals(last - 16, store.view().historyStart)
    }

    // ---- バックグラウンドの動作中 ----

    private val beta = ProjectInfo("beta", "beta", "/tmp/beta", open = true, running = false)

    private fun beta(seq: Long, e: ApiEvent): ApiEvent = e.copy(seq = seq, project = "beta")

    private fun AppStore.running(id: String): Boolean? = state.value.projects.find { it.id == id }?.running

    private fun listCalls() = core.callsOf<ApiCommand.ListProjects>().size

    @Test
    fun rereadsTheProjectListWhenATurnStartsOrEndsInAProjectThatIsNotOnScreen() = runTest {
        core.others = listOf(beta)
        core.log = log
        val store = newStore(core)
        openAndSettle(store)
        store.refreshProjects()
        assertEquals(false, store.running("beta"))
        val before = store.view()
        val listed = listCalls()

        core.others = listOf(beta.copy(running = true))
        push(core, beta(1, prompted(1, ORCHESTRATOR)))
        assertEquals(listed, listCalls()) // すぐではない: 変化はまとまって来る
        advanceTimeBy(250)
        runCurrent()
        assertEquals(true, store.running("beta"))
        assertEquals(listed + 1, listCalls())

        core.others = listOf(beta.copy(running = false))
        push(core, beta(2, turnEnded(2, ORCHESTRATOR)))
        advanceTimeBy(250)
        runCurrent()
        assertEquals(false, store.running("beta"))
        assertSame(before, store.view()) // 表示中のプロジェクトは変わらない
    }

    @Test
    fun asksOnceForABurstOfChangesAndNotAtAllForOutputOrForTheProjectOnScreen() = runTest {
        core.others = listOf(beta)
        core.log = log
        val store = newStore(core)
        openAndSettle(store)
        val listed = listCalls()

        push(core, beta(1, agentText(1, ORCHESTRATOR, "hello")), beta(2, chunk(2, ORCHESTRATOR, "hel")))
        push(core, prompted(last + 1, ORCHESTRATOR)) // repo は表示中
        advanceTimeBy(400)
        runCurrent()
        assertEquals(listed, listCalls())

        push(core, beta(3, prompted(3, ORCHESTRATOR)), beta(4, turnEnded(4, ORCHESTRATOR, CANCELLED)), beta(5, prompted(5, ORCHESTRATOR)))
        advanceTimeBy(250)
        runCurrent()
        assertEquals(listed + 1, listCalls())
        advanceTimeBy(400)
        runCurrent()
        assertEquals(listed + 1, listCalls())
    }

    @Test
    fun alsoFollowsTheProjectsTheCoreOpenedItselfWhileNoneIsOnScreen() = runTest {
        core.others = listOf(beta)
        val store = newStore(core)
        store.refreshProjects()
        assertNull(store.state.value.project)
        core.others = listOf(beta.copy(running = true))
        push(core, beta(1, prompted(1, ORCHESTRATOR)))
        advanceTimeBy(250)
        runCurrent()
        assertEquals(true, store.running("beta"))
    }

    @Test
    fun keepsTheFlagOfTheProjectItLeavesAsItsViewShowedItAndFollowsItAfterwards() = runTest {        core.others = listOf(beta)
        val busy = FULL_RUN.end.sessions[0].copy(status = SessionStatus.Live, turnRunning = true)
        core.snapshot = FULL_RUN.start.copy(sessions = listOf(busy))
        core.info = PROJECT.copy(running = true) // コアもそう言う
        val store = newStore(core)
        openAndSettle(store)
        store.refreshProjects()
        assertEquals(true, store.running("repo"))
        assertEquals(false, store.running("beta"))

        // 離れる: リングは回り続ける (待機に見える瞬間が無い) ...
        core.snapshot = FULL_RUN.start.copy(sessions = emptyList())
        core.info = PROJECT.copy(running = false)
        var during: Boolean? = null
        core.before = { cmd -> if (cmd is ApiCommand.GetSnapshot) during = store.running("repo") }
        store.openProject(beta.path)
        assertEquals(true, during)

        // ... コアが止まったと言うまで
        advanceTimeBy(250)
        runCurrent()
        assertEquals(false, store.running("repo"))
    }

    @Test
    fun readsBranchStatusesWithGitAndReusesThemUntilStale() = runTest {
        val store = newStore(core)
        openAndSettle(store)
        runCurrent()
        assertEquals(1, core.callsOf<ApiCommand.GetBranchStatuses>().size)
        assertEquals(core.branchStatuses, store.state.value.git?.statuses)
        // 概要の読み直しでは `gh` を聞き直さない
        store.refreshGit()
        runCurrent()
        assertEquals(1, core.callsOf<ApiCommand.GetBranchStatuses>().size)
        store.refreshBranchStatuses(force = true)
        runCurrent()
        assertEquals(2, core.callsOf<ApiCommand.GetBranchStatuses>().size)
    }
}
