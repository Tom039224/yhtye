package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentError
import io.github.tom039224.yhtye.shared.api.generated.AgentEvent
import io.github.tom039224.yhtye.shared.api.generated.AgentOutput
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.InboxKind
import io.github.tom039224.yhtye.shared.api.generated.ResultOfStringOrAgentError
import io.github.tom039224.yhtye.shared.api.generated.Role
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.api.generated.TextKind
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

/** PC の `src/store/project.test.ts` の移植。 */
class ProjectViewTest {
    private fun fold(view: ProjectView, events: List<ApiEvent> = FULL_RUN.events): ProjectView = events.fold(view) { v, e ->
        when {
            e.live -> applyLive(v, e)
            e.seq == v.cursor + 1 -> applyDurable(v, e)
            else -> v
        }
    }

    // ---- project view ----

    @Test
    fun buildsTheOrchestratorConversationFromTheRecordedRun() {
        val view = fold(applySnapshot(newProjectView(PROJECT), FULL_RUN.start))
        assertEquals(FULL_RUN.end.seq, view.cursor)
        assertEquals(FULL_RUN.end.state, view.state)
        val items = view.transcripts.getValue(ORCHESTRATOR)
        assertEquals("Add hello.txt please", items.filterIsInstance<TranscriptItem.User>().first().text)
        val texts = items.filterIsInstance<TranscriptItem.Text>().map { it.textKind to it.text }
        assertEquals(TextKind.Thought to "The user wants a greeting file. One code task with a review.", texts[0])
        assertEquals(TextKind.Message to "I will create a group for this.", texts[1])
        val tool = items.filterIsInstance<TranscriptItem.Tool>().first()
        assertEquals(Triple("read-1", "Read README.md", "completed"), Triple(tool.id, tool.title, tool.status))
        assertTrue(items.any { it is TranscriptItem.Notice && it.inboxKind == InboxKind.GroupSettled })
        assertTrue(view.transcripts.getValue("T-1/implementer").any { it is TranscriptItem.Prompt })
        assertEquals(Streamed("", ""), view.streaming[ORCHESTRATOR])
    }

    @Test
    fun streamsChunksAndReplacesThemWithTheCoalescedText() {
        var v = newProjectView(PROJECT)
        v = applyLive(v, chunk(0, ORCHESTRATOR, "Hel"))
        v = applyLive(v, chunk(0, ORCHESTRATOR, "lo"))
        v = applyLive(v, chunk(0, ORCHESTRATOR, "hmm", thought = true))
        assertEquals(Streamed("Hello", "hmm"), v.streaming[ORCHESTRATOR])
        v = applyDurable(v, agentText(1, ORCHESTRATOR, "Hello"))
        assertEquals(Streamed("", "hmm"), v.streaming[ORCHESTRATOR])
        assertEquals(1, v.transcripts.getValue(ORCHESTRATOR).size)
        v = applyDurable(v, turnEnded(2, ORCHESTRATOR))
        assertEquals(Streamed("", ""), v.streaming[ORCHESTRATOR])
    }

    @Test
    fun ignoresMalformedUsageUpdatesAndShowsOtherLiveEventsWithoutFoldingThem() {
        var v = applySnapshot(newProjectView(PROJECT), FULL_RUN.start)
        val malformed = ev(0, ApiEventBody.Agent(ORCHESTRATOR, AgentEvent.Output(AgentOutput.Usage(JsonObject(emptyMap())))), live = true)
        assertSame(v, applyLive(v, malformed))
        val unsaved = ev(0, ApiEventBody.SessionFailed("T-1/implementer", "boom", retrying = false), live = true)
        v = applyLive(v, unsaved)
        assertEquals(0, v.cursor)
        val items = v.transcripts.getValue("T-1/implementer")
        assertEquals(1, items.size)
        assertTrue((items[0] as TranscriptItem.Lifecycle).error)
    }

    @Test
    fun doesNotFoldEventsAlreadyInTheSnapshotIntoState() {
        val snap = FULL_RUN.end
        val v = durable(FULL_RUN).fold(applySnapshot(newProjectView(PROJECT), snap), ::applyDurable)
        assertEquals(snap.state, v.state)
        assertEquals(snap.sessions, v.sessions)
        assertTrue(v.transcripts.getValue(ORCHESTRATOR).size > 3)
    }

    @Test
    fun recordsFailedTurnsAndCancellations() {
        var v = newProjectView(PROJECT)
        v = applyDurable(v, turnEnded(1, ORCHESTRATOR, CANCELLED))
        v = applyDurable(v, ev(2, ApiEventBody.Agent(ORCHESTRATOR, AgentEvent.TurnEnded(ResultOfStringOrAgentError.Err(AgentError.Closed)))))
        val turns = v.transcripts.getValue(ORCHESTRATOR).map { (it as TranscriptItem.Turn).let { t -> t.outcome to t.error } }
        assertEquals(listOf("cancelled" to false, "error (closed)" to true), turns)
    }

    // ---- a deleted chat ----

    private fun deleted(seq: Long, chat: String) = domain(seq, DomainEvent.ChatDeleted(chat))

    /** 両方のチャットを読み込み、C-2 を選択、C-1 に通知、それぞれのサブエージェントのセッションと本文。 */
    private fun twoChats(): ProjectView = applySnapshot(newProjectView(PROJECT), chatSnapshot()).copy(
        unread = setOf("C-1"),
        sessions = listOf(
            sessionRecord(orchestratorKey("C-1")),
            sessionRecord("T-1/implementer", SessionStatus.Stopped, role = Role.Implementer),
            sessionRecord("T-2/implementer", role = Role.Implementer),
        ),
        transcripts = mapOf(
            orchestratorKey("C-1") to listOf(TranscriptItem.Prompt(1, 1, "x")),
            "T-1/implementer" to listOf(TranscriptItem.Prompt(2, 2, "y")),
            orchestratorKey("C-2") to listOf(TranscriptItem.Prompt(3, 3, "z")),
        ),
        streaming = mapOf(orchestratorKey("C-1") to Streamed("par", "")),
    )

    @Test
    fun takesItsConversationItsTasksSessionsAndItsNotificationMarkWithIt() {
        val v = applyDurable(twoChats().copy(stateSeq = 0, cursor = 4), deleted(5, "C-1"))
        assertEquals(listOf("C-2"), v.chats.map { it.id })
        assertEquals(listOf("C-2"), v.state?.chats?.map { it.id })
        assertEquals(listOf("G-2"), v.state?.groups?.map { it.id })
        assertEquals(listOf("T-2/implementer"), v.sessions.map { it.sessionKey })
        assertEquals(setOf(orchestratorKey("C-2")), v.transcripts.keys)
        assertEquals(emptyMap(), v.streaming)
        assertEquals(emptySet(), v.unread)
        assertEquals("C-2", v.selectedChat)
    }

    @Test
    fun showsTheMostRecentlyUsedOtherChatWhenTheShownOneIsDeleted() {
        val v = applyDurable(twoChats().copy(selectedChat = "C-2", stateSeq = 0, cursor = 4), deleted(5, "C-2"))
        assertEquals("C-1", v.selectedChat)
        val last = applyDurable(v, deleted(6, "C-1"))
        assertNull(last.selectedChat)
        assertEquals(emptyList(), last.chats)
    }

    @Test
    fun canBeDroppedBeforeItsEventArrivesAndTheEventThenChangesNothingMore() {
        val early = removeChatNow(twoChats(), "C-1")
        assertEquals(CHAT_INFOS.filter { it.id != "C-1" }, early.chats)
        assertTrue(early.state?.chats.orEmpty().any { it.id == "C-1" })
        assertEquals(listOf("T-2/implementer"), early.sessions.map { it.sessionKey })
        val later = applyDurable(early.copy(stateSeq = 0, cursor = 4), deleted(5, "C-1"))
        assertEquals(listOf("C-2"), later.state?.chats?.map { it.id })
        assertEquals(early.sessions, later.sessions)
        assertEquals(early.transcripts, later.transcripts)
        assertEquals(later, forgetChat(later, null, "C-1"))
    }

    @Test
    fun keepsEachSessionsLatestContextUsageUntilANewSessionStarts() {
        var v = newProjectView(PROJECT)
        assertNull(v.contextUsage[ORCHESTRATOR])
        v = applyLive(v, usage(0, ORCHESTRATOR, 1000, 200000))
        v = applyLive(v, usage(0, ORCHESTRATOR, 84000, 200000, 1.5 to "USD"))
        assertEquals(ContextUsage(84000.0, 200000.0, ContextUsage.Cost(1.5, "USD")), v.contextUsage[ORCHESTRATOR])
        assertNull(v.transcripts[ORCHESTRATOR])
        v = applyDurable(v, sessionStarted(1, ORCHESTRATOR, resumed = true))
        assertEquals(84000.0, v.contextUsage[ORCHESTRATOR]?.used)
        v = applyDurable(v, sessionStarted(2, ORCHESTRATOR, resumed = false))
        assertNull(v.contextUsage[ORCHESTRATOR])
    }

    @Test
    fun marksASessionCompactingFromItsCompactPromptToTheEndOfThatTurn() {
        var v = newProjectView(PROJECT)
        v = applyDurable(v, prompted(1, ORCHESTRATOR, "[yhtye:user_message]\nhi"))
        assertTrue(ORCHESTRATOR !in v.compacting)
        v = applyDurable(v, turnEnded(2, ORCHESTRATOR))
        v = applyDurable(v, prompted(3, ORCHESTRATOR, "/compact"))
        assertTrue(ORCHESTRATOR in v.compacting)
        // 会話には出すが、ユーザーの発言としては出さない
        assertEquals(listOf(TranscriptItem.Compact::class), v.transcripts.getValue(ORCHESTRATOR).map { it::class })
        v = applyDurable(v, turnEnded(4, ORCHESTRATOR))
        assertTrue(ORCHESTRATOR !in v.compacting)
    }

    @Test
    fun forgetsADeletedChatsContextUsageAndCompaction() {
        var v = applySnapshot(newProjectView(PROJECT), chatSnapshot())
        v = applyLive(v, usage(0, ORCHESTRATOR, 5, 10))
        v = applyDurable(v, prompted(v.cursor + 1, ORCHESTRATOR, "/compact"))
        v = forgetChat(v, v.state, "C-1")
        assertEquals(emptyMap(), v.contextUsage)
        assertEquals(emptySet(), v.compacting)
    }

    // ---- resuming chats ----

    private fun stopped(seq: Long, chat: String) = ev(seq, ApiEventBody.SessionStopped(orchestratorKey(chat), suspended = false))

    private fun failed(seq: Long, chat: String, retrying: Boolean) =
        ev(seq, ApiEventBody.SessionFailed(orchestratorKey(chat), "boom", retrying))

    private fun snap(status: SessionStatus): ProjectView =
        applySnapshot(newProjectView(PROJECT), chatSnapshot()).copy(sessions = listOf(sessionRecord(orchestratorKey("C-1"), status)))

    @Test
    fun isMarkedUntilTheChatsSessionStartsOrStops() {
        val resuming = markResuming(snap(SessionStatus.Interrupted), "C-1", true)
        assertEquals(setOf("C-1"), resuming.resuming)
        val next = resuming.copy(cursor = 10, stateSeq = 10)
        assertEquals(emptySet(), applyDurable(next, sessionStarted(11, orchestratorKey("C-1"), resumed = true)).resuming)
        assertEquals(emptySet(), applyDurable(next, stopped(11, "C-1")).resuming)
        // 別のチャットのセッションや別の種類のイベントでは残る
        assertEquals(setOf("C-1"), applyDurable(next, sessionStarted(11, orchestratorKey("C-2"), resumed = true)).resuming)
        assertEquals(setOf("C-1"), applyDurable(next, agentText(11, orchestratorKey("C-1"), "x")).resuming)
    }

    @Test
    fun isClearedWhenTheStartFailsForGoodNotWhenARetryFollowsTheFailure() {
        val next = markResuming(snap(SessionStatus.Interrupted), "C-1", true).copy(cursor = 10, stateSeq = 10)
        // 再試行される (新しいセッションに切り替わる) 復元の失敗はまだ起動中
        assertEquals(setOf("C-1"), applyDurable(next, failed(11, "C-1", true)).resuming)
        // 最後の失敗で終わる (PC の「印の無い古いログ」のケースは、生成された型で retrying が必須なので無い)
        assertEquals(emptySet(), applyDurable(next, failed(11, "C-1", false)).resuming)
        // 別のチャット・サブエージェントの失敗では残る
        assertEquals(setOf("C-1"), applyDurable(next, failed(11, "C-2", false)).resuming)
        val other = ev(11, ApiEventBody.SessionFailed("T-1/implementer", "boom", retrying = false))
        assertEquals(setOf("C-1"), applyDurable(next, other).resuming)
    }

    @Test
    fun goesThroughTheWholeSequenceOfARestoreThatIsRetriedAndThenFails() {
        var v = markResuming(snap(SessionStatus.Interrupted), "C-1", true).copy(cursor = 10, stateSeq = 10)
        v = applyDurable(v, failed(11, "C-1", true)) // もう一度 (1/2)
        v = applyDurable(v, failed(12, "C-1", true)) // もう一度 (2/2)
        v = applyDurable(v, failed(13, "C-1", true)) // 新しいセッションで
        assertEquals(setOf("C-1"), v.resuming)
        v = applyDurable(v, failed(14, "C-1", false)) // 新しいセッションも起動できない
        assertEquals(emptySet(), v.resuming)
    }

    @Test
    fun isClearedByTheCoresAnswerAndNotSetForASessionThatIsAlreadyUp() {
        val resuming = markResuming(snap(SessionStatus.Interrupted), "C-1", true)
        assertEquals(emptySet(), markResuming(resuming, "C-1", false).resuming)
        assertEquals(emptySet(), markResuming(snap(SessionStatus.Live), "C-1", true).resuming)
        val same = markResuming(snap(SessionStatus.Interrupted), "C-1", false)
        assertSame(same, markResuming(same, "C-1", false))
    }

    @Test
    fun goesWithADeletedChat() {
        val resuming = markResuming(snap(SessionStatus.Interrupted), "C-1", true)
        assertEquals(emptySet(), forgetChat(resuming, resuming.state, "C-1").resuming)
    }
}
