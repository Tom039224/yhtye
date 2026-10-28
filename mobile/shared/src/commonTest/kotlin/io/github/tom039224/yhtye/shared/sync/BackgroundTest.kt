package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.InboxKind
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.SessionRecord
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.api.generated.State
import io.github.tom039224.yhtye.shared.client.ConnectionState
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * 裏での接続 (常駐) と通知の判断: 作業中か ([hasWork])、保つか ([shouldKeepAlive]・猶予)、通知を出すか ([shouldNotify])。
 * Foreground Service と NotificationManager の実体は androidApp 側。
 */
@OptIn(ExperimentalCoroutinesApi::class)
class BackgroundTest {
    private val orchestrator = orchestratorKey("C-1")

    private fun view(
        groups: List<GroupStatus> = emptyList(),
        sessions: List<SessionRecord> = emptyList(),
        extra: (State) -> State = { it },
    ): ProjectView {
        val base = chatState()
        val state = base.copy(groups = base.groups.mapIndexed { i, g -> g.copy(status = groups.getOrElse(i) { GroupStatus.Done }) })
        return ProjectView(PROJECT, phase = LoadPhase.Ready, state = extra(state), sessions = sessions)
    }

    private fun app(view: ProjectView? = null, projects: List<ProjectInfo> = listOf(PROJECT)) =
        AppState(connection = ConnectionState.Connected(1), projects = projects, projectsLoaded = true, project = view)

    // ---- 作業中か ----

    @Test
    fun hasNoWorkWhenNothingRunsAndEveryGroupIsOverOrBlocked() {
        assertFalse(hasWork(app(view())))
        // マージ保留はユーザーが整えるまで止まる: 作業ではない
        assertFalse(hasWork(app(view(groups = listOf(GroupStatus.MergeBlocked, GroupStatus.Cancelled)))))
        assertFalse(hasWork(app(null)))
    }

    @Test
    fun hasWorkWhileAGroupIsActiveOrFinishing() {
        assertTrue(hasWork(app(view(groups = listOf(GroupStatus.Done, GroupStatus.Active)))))
        assertTrue(hasWork(app(view(groups = listOf(GroupStatus.Finishing, GroupStatus.Done)))))
    }

    @Test
    fun hasWorkWhileATurnIsRunning() {
        val running = sessionRecord(orchestrator, SessionStatus.Live, turnRunning = true)
        assertTrue(hasWork(app(view(sessions = listOf(running)))))
        // 止まったセッションの turnRunning は残骸
        assertFalse(hasWork(app(view(sessions = listOf(running.copy(status = SessionStatus.Stopped))))))
    }

    @Test
    fun hasWorkWhileALiveOrchestratorHasSomethingInItsInboxButNotWithoutOne() {
        val withInbox = { v: ProjectView ->
            v.copy(state = applyDomainEvent(v.state!!, (inbox(9, 7, "C-1", InboxKind.HelpRaised, "x").body as ApiEventBody.Domain).event))
        }
        val live = sessionRecord(orchestrator, SessionStatus.Live)
        assertTrue(hasWork(app(withInbox(view(sessions = listOf(live))))))
        // 起きていないオーケストレータ宛の項目では常駐しない (いつまでも終わらない)
        assertFalse(hasWork(app(withInbox(view(sessions = emptyList())))))
        assertFalse(hasWork(app(withInbox(view(sessions = listOf(live.copy(status = SessionStatus.Stopped)))))))
    }

    @Test
    fun hasWorkWhenAnotherProjectIsRunningButOnlyAnOpenOne() {
        val other = ProjectInfo("other", "other", "/tmp/other", open = true, running = true, awake = true)
        assertTrue(hasWork(app(view(), listOf(PROJECT, other))))
        assertFalse(hasWork(app(view(), listOf(PROJECT, other.copy(running = false)))))
        // 開いていないプロジェクトの running は残骸
        assertFalse(hasWork(app(view(), listOf(PROJECT, other.copy(open = false)))))
        // 表示中のプロジェクト自身の一覧の値は見ない (ビューが言う)
        assertFalse(hasWork(app(view(), listOf(PROJECT.copy(running = true)))))
    }

    @Test
    fun usesTheProjectListWhileTheProjectIsStillLoading() {
        val loading = ProjectView(PROJECT, phase = LoadPhase.Loading)
        assertTrue(hasWork(app(loading, listOf(PROJECT.copy(running = true)))))
        assertFalse(hasWork(app(loading, listOf(PROJECT))))
    }

    // ---- 保つか ----

    private fun keep(
        mode: BackgroundMode,
        work: Boolean = false,
        paired: Boolean = true,
        connection: ConnectionState? = ConnectionState.Connected(1),
    ) = shouldKeepAlive(KeepAliveInput(paired, mode, connection, work))

    @Test
    fun alwaysModeKeepsTheConnectionWhilePairedWithOrWithoutWork() {
        assertTrue(keep(BackgroundMode.Always))
        assertTrue(keep(BackgroundMode.Always, work = true))
        assertTrue(keep(BackgroundMode.Always, connection = ConnectionState.WaitingToReconnect(2, 4_000, "gone")))
        assertTrue(keep(BackgroundMode.Always, connection = ConnectionState.Idle))
        assertFalse(keep(BackgroundMode.Always, paired = false))
    }

    @Test
    fun workOnlyModeKeepsTheConnectionOnlyWhileThereIsWork() {
        assertFalse(keep(BackgroundMode.WorkOnly))
        assertTrue(keep(BackgroundMode.WorkOnly, work = true))
        assertFalse(keep(BackgroundMode.WorkOnly, work = true, paired = false))
    }

    @Test
    fun stopsForARevokedOrRejectedDeviceWhateverTheMode() {
        for (mode in BackgroundMode.entries) {
            assertFalse(keep(mode, work = true, connection = ConnectionState.Revoked))
            assertFalse(keep(mode, work = true, connection = ConnectionState.Rejected("403")))
        }
    }

    @Test
    fun readsTheSavedModeAndFallsBackToAlways() {
        assertEquals(BackgroundMode.WorkOnly, BackgroundMode.parse("work_only"))
        assertEquals(BackgroundMode.Always, BackgroundMode.parse("always"))
        assertEquals(BackgroundMode.Always, BackgroundMode.parse(null))
        assertEquals(BackgroundMode.Always, BackgroundMode.parse("something new"))
    }

    // ---- 猶予 ----

    @Test
    fun holdsAFallForTheGraceButRisesAtOnceAndForgivesAShortGap() = runTest {
        val input = MutableStateFlow(false)
        val seen = mutableListOf<Boolean>()
        backgroundScope.launch { input.holdOnFall(1_000).collect { seen += it } }
        runCurrent()
        // 最初の false も猶予の後
        advanceTimeBy(999)
        assertEquals(emptyList(), seen)
        advanceTimeBy(2)
        assertEquals(listOf(false), seen)
        input.value = true
        runCurrent()
        assertEquals(listOf(false, true), seen)
        // 猶予の中で戻れば何も起きない
        input.value = false
        advanceTimeBy(500)
        input.value = true
        advanceTimeBy(2_000)
        assertEquals(listOf(false, true), seen)
        input.value = false
        advanceTimeBy(1_001)
        assertEquals(listOf(false, true, false), seen)
    }

    // ---- 制御 (常駐と通知) ----

    private class FakeNotifier : TurnNotifier {
        val posted = mutableListOf<UsersTurn>()
        val cancelled = mutableListOf<Pair<String, String>>()

        override fun post(turn: UsersTurn) {
            posted += turn
        }

        override fun cancel(project: String, chat: String) {
            cancelled += project to chat
        }
    }

    private class Rig(
        scope: kotlinx.coroutines.CoroutineScope,
        mode: BackgroundMode,
        initial: AppState,
    ) {
        val mode = MutableStateFlow(mode)
        val connection = MutableStateFlow<ConnectionState>(ConnectionState.Connected(1))
        val app = MutableStateFlow(initial)
        val foreground = MutableStateFlow(false)
        val turns = MutableSharedFlow<UsersTurn>(extraBufferCapacity = 16)
        val notifier = FakeNotifier()
        val controller = BackgroundController(scope, this.mode, connection, app, foreground, turns, notifier, graceMs = 3_000)
    }

    private val idle = app(view())
    private val working = app(view(groups = listOf(GroupStatus.Done, GroupStatus.Active)))
    private val turn = UsersTurn("repo", "C-1", "Refactor checkout", "Done.\nMore", 5)

    @Test
    fun alwaysModeRunsTheServiceAndStaysConnectedInTheBackgroundWithoutAnyWork() = runTest {
        val r = Rig(backgroundScope, BackgroundMode.Always, idle)
        runCurrent()
        assertTrue(r.controller.keepAlive.value)
        assertTrue(r.controller.wantConnected.value)
        advanceTimeBy(60_000)
        assertTrue(r.controller.keepAlive.value)
    }

    @Test
    fun workOnlyModeRunsThroughTheWorkAndStopsAfterTheGraceOnceItIsOver() = runTest {
        val r = Rig(backgroundScope, BackgroundMode.WorkOnly, idle)
        runCurrent()
        assertFalse(r.controller.keepAlive.value)
        assertFalse(r.controller.wantConnected.value)
        // 作業が始まった (前面で)
        r.foreground.value = true
        r.app.value = working
        runCurrent()
        assertTrue(r.controller.keepAlive.value)
        // 裏に回っても続く
        r.foreground.value = false
        runCurrent()
        assertTrue(r.controller.wantConnected.value)
        // 終わった: ユーザーの番の通知を出し、猶予の後に止める
        r.app.value = idle
        r.turns.tryEmit(turn)
        runCurrent()
        assertEquals(listOf(turn), r.notifier.posted)
        assertTrue(r.controller.keepAlive.value)
        advanceTimeBy(2_999)
        assertTrue(r.controller.keepAlive.value)
        advanceTimeBy(2)
        assertFalse(r.controller.keepAlive.value)
        assertFalse(r.controller.wantConnected.value)
    }

    @Test
    fun workOnlyModeKeepsRunningThroughAShortGapBetweenTurns() = runTest {
        val r = Rig(backgroundScope, BackgroundMode.WorkOnly, working)
        runCurrent()
        assertTrue(r.controller.keepAlive.value)
        r.app.value = idle
        advanceTimeBy(1_000)
        r.app.value = working
        advanceTimeBy(10_000)
        assertTrue(r.controller.keepAlive.value)
    }

    @Test
    fun theForegroundWantsTheConnectionEvenWhenNothingKeepsItAlive() = runTest {
        val r = Rig(backgroundScope, BackgroundMode.WorkOnly, idle)
        r.foreground.value = true
        runCurrent()
        assertFalse(r.controller.keepAlive.value)
        assertTrue(r.controller.wantConnected.value)
        r.foreground.value = false
        runCurrent()
        assertFalse(r.controller.wantConnected.value)
    }

    @Test
    fun switchingTheModeStartsOrStopsTheServiceDecision() = runTest {
        val r = Rig(backgroundScope, BackgroundMode.Always, idle)
        runCurrent()
        assertTrue(r.controller.keepAlive.value)
        // 常に → 作業中だけ: 作業が無ければ猶予の後に止まる
        r.mode.value = BackgroundMode.WorkOnly
        runCurrent()
        assertTrue(r.controller.keepAlive.value)
        advanceTimeBy(3_001)
        assertFalse(r.controller.keepAlive.value)
        // 作業中だけ → 常に: すぐ動く
        r.mode.value = BackgroundMode.Always
        runCurrent()
        assertTrue(r.controller.keepAlive.value)
        // 作業中なら作業中だけでも止まらない
        r.app.value = working
        r.mode.value = BackgroundMode.WorkOnly
        advanceTimeBy(10_000)
        assertTrue(r.controller.keepAlive.value)
    }

    @Test
    fun aRevokedDeviceStopsTheServiceEvenInAlwaysMode() = runTest {
        val r = Rig(backgroundScope, BackgroundMode.Always, idle)
        runCurrent()
        r.connection.value = ConnectionState.Revoked
        advanceTimeBy(3_001)
        assertFalse(r.controller.keepAlive.value)
        // 再接続が働かない拒否 (403) も
        r.connection.value = ConnectionState.Connected(2)
        runCurrent()
        assertTrue(r.controller.keepAlive.value)
        r.connection.value = ConnectionState.Rejected("whois")
        advanceTimeBy(3_001)
        assertFalse(r.controller.keepAlive.value)
    }

    // ---- 通知を出すか ----

    @Test
    fun notifiesInTheBackgroundButNotWhileTheChatIsOnScreenInTheForeground() = runTest {
        val onScreen = app(view().copy(selectedChat = "C-1"))
        val r = Rig(backgroundScope, BackgroundMode.Always, onScreen)
        runCurrent()
        r.turns.tryEmit(turn)
        runCurrent()
        assertEquals(listOf(turn), r.notifier.posted)
        r.foreground.value = true
        runCurrent()
        r.turns.tryEmit(turn.copy(tsMs = 6))
        runCurrent()
        assertEquals(1, r.notifier.posted.size)
    }

    @Test
    fun notifiesForAnotherChatOrProjectEvenInTheForeground() = runTest {
        val onScreen = app(view().copy(selectedChat = "C-1"))
        val r = Rig(backgroundScope, BackgroundMode.Always, onScreen)
        r.foreground.value = true
        runCurrent()
        val otherChat = turn.copy(chat = "C-2")
        val otherProject = turn.copy(project = "beta")
        r.turns.tryEmit(otherChat)
        r.turns.tryEmit(otherProject)
        runCurrent()
        assertEquals(listOf(otherChat, otherProject), r.notifier.posted)
    }

    @Test
    fun notifiesWhileTheAppIsInTheForegroundOnTheRunsViewWithNoChatSelected() = runTest {
        val r = Rig(backgroundScope, BackgroundMode.Always, app(null))
        r.foreground.value = true
        runCurrent()
        r.turns.tryEmit(turn)
        runCurrent()
        assertEquals(listOf(turn), r.notifier.posted)
    }

    @Test
    fun clearsTheNotificationOfAChatOnceItIsOnScreenInTheForeground() = runTest {
        val r = Rig(backgroundScope, BackgroundMode.Always, app(view().copy(selectedChat = "C-2")))
        runCurrent()
        // 裏にいる間は消さない
        assertEquals(emptyList(), r.notifier.cancelled)
        r.foreground.value = true
        runCurrent()
        assertEquals(listOf("repo" to "C-2"), r.notifier.cancelled)
        // チャットを替えたら、そのチャットのも
        r.app.value = app(view().copy(selectedChat = "C-1"))
        runCurrent()
        assertEquals(listOf("repo" to "C-2", "repo" to "C-1"), r.notifier.cancelled)
    }

    @Test
    fun shouldNotifyIsTheSameDecisionAsTheController() {
        val shown = DisplayContext(foreground = true, project = "repo", chat = "C-1")
        assertFalse(shouldNotify(turn, shown))
        assertTrue(shouldNotify(turn, shown.copy(foreground = false)))
        assertTrue(shouldNotify(turn, shown.copy(chat = "C-2")))
        assertTrue(shouldNotify(turn, shown.copy(project = "beta")))
        assertTrue(shouldNotify(turn, shown.copy(project = null, chat = null)))
    }

    // ---- 通知の文 ----

    @Test
    fun theTextIsTheChatTitleAndTheFirstLineOfTheLastWords() {
        assertEquals(
            NotificationText("Refactor checkout の作業が完了しました", "Done."),
            usersTurnText(turn, untitled = "新しいチャット"),
        )
        assertEquals("新しいチャット の作業が完了しました", usersTurnText(turn.copy(title = null), "新しいチャット").title)
        assertNull(usersTurnText(turn.copy(lastMessage = null), "新しいチャット").body)
        assertNull(usersTurnText(turn.copy(lastMessage = "  \n"), "新しいチャット").body)
        // 長い 1 行は切る
        val long = usersTurnText(turn.copy(lastMessage = "あ".repeat(200)), "x").body.orEmpty()
        assertTrue(long.length < 200 && long.endsWith("…"))
    }

    // ---- 本物の同期層と ----

    private val core = FakeCore(PROJECT, chatSnapshot()).apply { log = CHAT_LOG }
    private var seq = CHAT_LOG.size.toLong()

    private fun TestScope.finishTurn(chat: String, said: String) {
        val key = orchestratorKey(chat)
        val events = arrayOf(prompted(++seq, key), agentText(++seq, key, said), turnEnded(++seq, key))
        core.log = core.log + events
        push(core, *events)
    }

    @Test
    fun aFinishedTurnInTheStoreBecomesANotificationUnlessTheChatIsOnScreenAndNotForHelp() = runTest {
        val store = newStore(core)
        val foreground = MutableStateFlow(false)
        val notifier = FakeNotifier()
        BackgroundController(backgroundScope, MutableStateFlow(BackgroundMode.Always), MutableStateFlow(ConnectionState.Connected(1)), store.state, foreground, store.usersTurns, notifier)
        openAndSettle(store)
        store.selectChat("C-1")
        runCurrent()
        // help の通知が受信箱に積まれたターンの終わりでは出さない
        push(core, inbox(++seq, 9, "C-1", InboxKind.HelpRaised, "stuck"), prompted(++seq, orchestratorKey("C-1")), turnEnded(++seq, orchestratorKey("C-1")))
        assertEquals(emptyList(), notifier.posted)
        push(core, delivered(++seq, 9, "C-1"))
        // 前面で C-1 を見ている: 出さない
        foreground.value = true
        runCurrent()
        finishTurn("C-1", "Handled the help.")
        assertEquals(emptyList(), notifier.posted)
        // 裏に回ったら出す
        foreground.value = false
        runCurrent()
        finishTurn("C-1", "All done.\nSee the branch.")
        assertEquals(listOf("C-1" to "All done.\nSee the branch."), notifier.posted.map { it.chat to it.lastMessage })
    }

    // ---- 通知のタップ ----

    @Test
    fun showChatOpensTheChatOfTheLoadedProjectAndAnotherProject() = runTest {
        val other = ProjectInfo("beta", "beta", "/tmp/beta", open = false, running = false, awake = false)
        core.others = listOf(other)
        val store = newStore(core)
        openAndSettle(store)
        assertTrue(store.showChat("repo", "C-2"))
        runCurrent()
        assertEquals("C-2", store.view().selectedChat)
        assertTrue(store.showChat("repo", "C-1"))
        runCurrent()
        assertEquals("C-1", store.view().selectedChat)
        // 選んだチャットはコアに知らせる (オーケストレータを戻すため)
        assertEquals(listOf("C-2", "C-1"), core.callsOf<ApiCommand.OpenChat>().map { it.chat }.takeLast(2))
        // 別のプロジェクトのチャット (同じ記録のスナップショットで答える)
        assertTrue(store.showChat("beta", "C-2"))
        runCurrent()
        assertEquals("beta", store.view().info.id)
        assertEquals("C-2", store.view().selectedChat)
    }

    @Test
    fun showChatSaysNoForAnUnknownProjectOrAChatThatWasDeleted() = runTest {
        val store = newStore(core)
        openAndSettle(store)
        assertFalse(store.showChat("nowhere", "C-1"))
        assertFalse(store.showChat("repo", "C-99"))
        assertEquals("repo", store.view().info.id)
    }

    @Test
    fun showChatWaitsForTheConnectionAndTheProjectListWhenTheAppWasStartedByTheTap() = runTest {
        core.state.value = ConnectionState.Connecting(0)
        val store = newStore(core)
        val result = mutableListOf<Boolean>()
        backgroundScope.launch { result += store.showChat("repo", "C-2") }
        runCurrent()
        assertEquals(emptyList(), result)
        core.state.value = ConnectionState.Connected(1)
        runCurrent()
        assertEquals(listOf(true), result)
        assertEquals("C-2", store.view().selectedChat)
    }
}
