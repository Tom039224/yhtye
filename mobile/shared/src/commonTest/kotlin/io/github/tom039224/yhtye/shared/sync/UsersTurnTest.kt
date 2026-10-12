package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentError
import io.github.tom039224.yhtye.shared.api.generated.AgentEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.InboxKind
import io.github.tom039224.yhtye.shared.api.generated.ResultOfStringOrAgentError
import io.github.tom039224.yhtye.shared.api.generated.Role
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.client.ConnectionState
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * 「オーケストレータの手が止まりユーザーの番になった」の検知: ターンが正常に終わり、そのチャットの受信箱が空で、
 * 走っているグループ (active / finishing) が無い。merge_blocked は含む。
 */
@OptIn(ExperimentalCoroutinesApi::class)
class UsersTurnTest {
    private val core = FakeCore(PROJECT, chatSnapshot()).apply { log = CHAT_LOG }
    private val c1 = orchestratorKey("C-1")
    private val c2 = orchestratorKey("C-2")

    /** 開いたストアと、そのユーザーの番の知らせ。 */
    private suspend fun TestScope.opened(): Pair<AppStore, List<UsersTurn>> {
        val s = newStore(core)
        val turns = collected(s.usersTurns)
        openAndSettle(s)
        return s to turns
    }

    /** 続きのイベントを (ログにも足して) 押し込む。 */
    private fun TestScope.happen(vararg events: ApiEvent) {
        core.log = core.log + events.filter { !it.live }
        push(core, *events)
    }

    private var seq = CHAT_LOG.size.toLong()

    private fun next(): Long = ++seq

    /** [chat] の 1 ターン: プロンプト → 発言 → 終わり。 */
    private fun turn(chat: String, said: String?, end: String = END_TURN): Array<ApiEvent> {
        val key = orchestratorKey(chat)
        return listOfNotNull(
            prompted(next(), key),
            said?.let { agentText(next(), key, it) },
            turnEnded(next(), key, end),
        ).toTypedArray()
    }

    // ---- 純関数の条件 ----

    @Test
    fun isTheUsersTurnOnlyWithAnEmptyInboxNoRunningGroupAndNoTurnOn() {
        val state = chatState()
        assertTrue(isUsersTurn(state, emptyList(), "C-1"))
        // C-2 のグループは進行中
        assertFalse(isUsersTurn(state, emptyList(), "C-2"))
        val group = { status: GroupStatus -> state.copy(groups = state.groups.map { if (it.id == "G-2") it.copy(status = status) else it }) }
        assertFalse(isUsersTurn(group(GroupStatus.Finishing), emptyList(), "C-2"))
        assertTrue(isUsersTurn(group(GroupStatus.MergeBlocked), emptyList(), "C-2"))
        assertTrue(isUsersTurn(group(GroupStatus.Done), emptyList(), "C-2"))
        assertTrue(isUsersTurn(group(GroupStatus.Cancelled), emptyList(), "C-2"))
        // 受信箱にそのチャットの項目があればオーケストレータがまた起きる (他のチャットの項目は関係ない)
        val queued = applyDomainEvent(state, (inbox(9, 7, "C-1", InboxKind.HelpRaised, "x").body as ApiEventBody.Domain).event)
        assertFalse(isUsersTurn(queued, emptyList(), "C-1"))
        val other = applyDomainEvent(state, (inbox(9, 7, "C-2", InboxKind.UserMessage, "x").body as ApiEventBody.Domain).event)
        assertTrue(isUsersTurn(other, emptyList(), "C-1"))
        // 次のターンがもう始まっている
        assertFalse(isUsersTurn(state, listOf(sessionRecord(c1, SessionStatus.Live, turnRunning = true)), "C-1"))
        assertTrue(isUsersTurn(state, listOf(sessionRecord(c1, SessionStatus.Stopped, turnRunning = true)), "C-1"))
        // 消えたチャット
        assertFalse(isUsersTurn(state, emptyList(), "C-9"))
    }

    // ---- 表示中のプロジェクト ----

    @Test
    fun saysItIsTheUsersTurnWhenATurnEndsNormallyWithNothingElseToDo() = runTest {
        val (_, turns) = opened()
        val events = turn("C-1", "Done: hello.txt is on main.\nMore details")
        happen(*events)
        assertEquals(
            listOf(UsersTurn("repo", "C-1", "Refactor checkout", "Done: hello.txt is on main.\nMore details", events.last().tsMs)),
            turns,
        )
        assertEquals("Done: hello.txt is on main.", firstLine(turns.single().lastMessage.orEmpty()))
    }

    @Test
    fun hasNoMessageWhenTheOrchestratorSaidNothingInThatTurn() = runTest {
        val (_, turns) = opened()
        // 前のターンの返事 (CHAT_LOG) はこのターンのものではない
        happen(userMessage(next(), 3, "again", "C-1"), delivered(next(), 3, "C-1"), *turn("C-1", said = null))
        assertEquals(listOf<String?>(null), turns.map { it.lastMessage })
    }

    @Test
    fun saysNothingWhileAGroupOfTheChatIsRunningButDoesOnceItsMergeIsBlocked() = runTest {
        val (_, turns) = opened()
        happen(*turn("C-2", "Tasks are running."))
        assertEquals(emptyList(), turns)
        happen(domain(next(), DomainEvent.GroupFinishing("G-2", "sum")), *turn("C-2", "Merging."))
        assertEquals(emptyList(), turns)
        happen(domain(next(), DomainEvent.GroupMergeFinished("G-2", ok = false, detail = "conflict")), *turn("C-2", "The merge is blocked."))
        assertEquals(listOf("C-2" to "The merge is blocked."), turns.map { it.chat to it.lastMessage })
    }

    @Test
    fun saysNothingWhileTheChatsInboxHasSomethingForTheOrchestrator() = runTest {
        val (_, turns) = opened()
        // ターン中に届いた通知はターンの後で配られる: まだユーザーの番ではない
        happen(prompted(next(), c1), inbox(next(), 3, "C-1", InboxKind.HelpRaised, "stuck"), turnEnded(next(), c1))
        assertEquals(emptyList(), turns)
        // 配られて、そのターンが終わったら
        happen(delivered(next(), 3, "C-1"), *turn("C-1", "Answered the help."))
        assertEquals(listOf("C-1"), turns.map { it.chat })
    }

    @Test
    fun ignoresAnotherChatsInbox() = runTest {
        val (_, turns) = opened()
        happen(userMessage(next(), 3, "for C-2", "C-2"), *turn("C-1", "Done."))
        assertEquals(listOf("C-1"), turns.map { it.chat })
    }

    @Test
    fun saysNothingForACancelledOrFailedTurnASubAgentOrACompaction() = runTest {
        val (_, turns) = opened()
        happen(*turn("C-1", "Stopping.", end = CANCELLED))
        happen(prompted(next(), c1), ev(next(), ApiEventBody.Agent(c1, AgentEvent.TurnEnded(ResultOfStringOrAgentError.Err(AgentError.Closed)))))
        happen(prompted(next(), "T-1/implementer"), turnEnded(next(), "T-1/implementer"))
        happen(prompted(next(), c1, COMPACT_PROMPT), turnEnded(next(), c1))
        assertEquals(emptyList(), turns)
        // 圧縮の後の普通のターンは数える
        happen(*turn("C-1", "Back."))
        assertEquals(listOf("Back."), turns.map { it.lastMessage })
    }

    @Test
    fun ignoresTurnsThatEndedBeforeTheSnapshotWhenTheProjectLoads() = runTest {
        // 履歴として読むターンの終わり (スナップショットに入っている) は今のことではない
        val old = turnEnded(5, c1)
        core.log = CHAT_LOG + old
        core.snapshot = chatSnapshot().copy(seq = 5)
        val (s, turns) = opened()
        assertEquals(5, s.view().cursor)
        assertEquals(emptyList(), turns)
    }

    @Test
    fun reportsATurnThatEndedWhileDisconnectedOnceAfterCatchingUp() = runTest {
        val (_, turns) = opened()
        core.state.value = ConnectionState.WaitingToReconnect(1, 1_000, "gone")
        runCurrent()
        core.log = core.log + turn("C-1", "Finished while you were away.")
        core.state.value = ConnectionState.Connected(2)
        runCurrent()
        assertEquals(listOf("Finished while you were away."), turns.map { it.lastMessage })
        // 遅れて届いた同じイベントでもう一度は言わない
        push(core, *core.log.takeLast(3).toTypedArray())
        assertEquals(1, turns.size)
    }

    // ---- 表示していないプロジェクト ----

    private fun beta(e: ApiEvent): ApiEvent = e.copy(project = "beta")

    @Test
    fun checksAProjectNotOnScreenWithASnapshotWhenItsTurnEnds() = runTest {
        val s = newStore(core)
        val turns = collected(s.usersTurns)
        val events = turn("C-1", "Background work done.").map(::beta).toTypedArray()
        push(core, *events)
        assertEquals(listOf(ApiCommand.GetSnapshot("beta")), core.callsOf<ApiCommand.GetSnapshot>())
        assertEquals(listOf(UsersTurn("beta", "C-1", "Refactor checkout", "Background work done.", events.last().tsMs)), turns)
    }

    @Test
    fun saysNothingForAProjectNotOnScreenWhoseSnapshotShowsWorkToDo() = runTest {
        val s = newStore(core)
        val turns = collected(s.usersTurns)
        // C-2 のグループは進行中
        push(core, *turn("C-2", "Still running.").map(::beta).toTypedArray())
        // 次のターンがもう始まっている
        core.snapshot = chatSnapshot().copy(sessions = listOf(sessionRecord(c1, SessionStatus.Live, turnRunning = true)))
        push(core, *turn("C-1", "Then more.").map(::beta).toTypedArray())
        assertEquals(emptyList(), turns)
    }

    @Test
    fun doesNotAskForASnapshotForACompactionOrACancelledTurnInTheBackground() = runTest {
        val s = newStore(core)
        val turns = collected(s.usersTurns)
        push(core, beta(prompted(next(), c1, COMPACT_PROMPT)), beta(turnEnded(next(), c1)))
        push(core, *turn("C-1", "Stop.", end = CANCELLED).map(::beta).toTypedArray())
        push(core, beta(prompted(next(), "T-1/implementer")), beta(turnEnded(next(), "T-1/implementer")))
        assertEquals(emptyList(), core.callsOf<ApiCommand.GetSnapshot>())
        assertEquals(emptyList(), turns)
    }

    @Test
    fun usesTheViewOnceTheProjectIsOnScreen() = runTest {
        // 表示中のプロジェクトはビューで判断し、スナップショットを取り直さない
        val (_, turns) = opened()
        val snapshots = core.callsOf<ApiCommand.GetSnapshot>().size
        happen(sessionStarted(next(), c1, role = Role.Orchestrator), *turn("C-1", "Done."))
        assertEquals(snapshots, core.callsOf<ApiCommand.GetSnapshot>().size)
        assertEquals(1, turns.size)
    }
}
