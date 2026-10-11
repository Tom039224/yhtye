package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiError
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.InboxKind
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.client.ApiException
import io.github.tom039224.yhtye.shared.client.ConnectionState
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** PC の `src/store/appChats.test.ts` の移植 (チャットの選択と記憶、選択中のチャットへの送信、チャットとブランチの作成、ping)。 */
@OptIn(ExperimentalCoroutinesApi::class)
class AppStoreChatsTest {
    private val core = FakeCore(PROJECT, chatSnapshot()).apply { log = CHAT_LOG }
    private val prefs = MemoryPrefs()

    private fun openChats() = core.callsOf<ApiCommand.OpenChat>()

    // ---- chat selection ----

    @Test
    fun opensTheMostRecentlyUsedChat() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertEquals("C-2", s.view().selectedChat)
    }

    @Test
    fun remembersTheSelectionPerProjectAndRestoresItWhenTheProjectIsOpenedAgain() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        s.selectChat("C-1")
        runCurrent()
        assertEquals("C-1", prefs.get(chatPrefKey(PROJECT.id)))
        val again = newStore(core, prefs)
        openAndSettle(again)
        assertEquals("C-1", again.view().selectedChat)
        // 覚えていたチャットが無ければ最近使ったもの
        prefs.set(chatPrefKey(PROJECT.id), "C-9")
        val third = newStore(core, prefs)
        openAndSettle(third)
        assertEquals("C-2", third.view().selectedChat)
    }

    @Test
    fun ignoresAChatTheProjectDoesNotHave() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        s.selectChat("C-9")
        runCurrent()
        assertEquals("C-2", s.view().selectedChat)
    }

    @Test
    fun hasNoSelectedChatInAProjectWithoutChats() = runTest {
        core.snapshot = chatSnapshot(emptyList(), core.snapshot.state.copy(chats = emptyList(), groups = emptyList(), tasks = emptyList()))
        core.log = emptyList()
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertNull(s.view().selectedChat)
        assertFalse(s.sendMessage("hi"))
        s.cancelTurn()
        assertEquals(0, core.callsOf<ApiCommand.SendUserMessage>().size)
        // チャットを勝手に作らない
        assertEquals(0, core.callsOf<ApiCommand.CreateChat>().size)
    }

    // ---- opening a chat brings its orchestrator back ----

    @Test
    fun asksTheCoreToOpenTheChatThatIsShownWhenTheProjectLoads() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertEquals(listOf(ApiCommand.OpenChat("repo", "C-2")), openChats())
    }

    @Test
    fun asksAgainForEachChatThatIsSelectedButNotForTheOneAlreadyShown() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        s.selectChat("C-2")
        s.selectChat("C-1")
        runCurrent()
        assertEquals(listOf(ApiCommand.OpenChat("repo", "C-2"), ApiCommand.OpenChat("repo", "C-1")), openChats())
        // 知らないチャットは選ばれないので何も聞かない
        s.selectChat("C-9")
        runCurrent()
        assertEquals(2, openChats().size)
    }

    @Test
    fun showsTheChatAsStartingUntilItsSessionIsUp() = runTest {
        core.starting += "C-1"
        val s = newStore(core, prefs)
        openAndSettle(s)
        s.selectChat("C-1")
        runCurrent()
        assertEquals(setOf("C-1"), s.view().resuming)
        push(core, sessionStarted(5, orchestratorKey("C-1"), resumed = true))
        assertEquals(emptySet(), s.view().resuming)
        assertEquals(SessionStatus.Live, s.view().sessions.find { it.sessionKey == orchestratorKey("C-1") }?.status)
    }

    @Test
    fun stopsShowingTheChatAsStartingWhenItsStartFailsForGoodNotWhenARetryFollows() = runTest {
        core.starting += "C-1"
        val s = newStore(core, prefs)
        openAndSettle(s)
        s.selectChat("C-1")
        runCurrent()
        assertEquals(setOf("C-1"), s.view().resuming)
        val failed = { seq: Long, retrying: Boolean -> ev(seq, ApiEventBody.SessionFailed(orchestratorKey("C-1"), "boom", retrying)) }
        push(core, failed(5, true))
        assertEquals(setOf("C-1"), s.view().resuming)
        push(core, failed(6, false))
        assertEquals(emptySet(), s.view().resuming)
    }

    @Test
    fun isNotStartingWhenTheCoreHasNothingToStart() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        s.selectChat("C-1")
        runCurrent()
        assertEquals(2, openChats().size)
        assertEquals(emptySet(), s.view().resuming)
    }

    @Test
    fun showsAnErrorWhenTheCoreRefusesToOpenTheChat() = runTest {
        core.fail(ApiCommand.OpenChat::class, ApiErrorCode.NotFound, "no chat C-2")
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertEquals(listOf("open_chat failed: no chat C-2"), s.state.value.errors.map { it.message })
        assertEquals(emptySet(), s.view().resuming)
    }

    // ---- transcripts per chat ----

    @Test
    fun keepsEachChatsConversationUnderItsOwnOrchestratorSession() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        val t = s.view().transcripts
        val kinds = { chat: String -> t.getValue(orchestratorKey(chat)).map { it::class } }
        assertEquals(listOf(TranscriptItem.User::class, TranscriptItem.Text::class), kinds("C-1"))
        assertEquals(listOf(TranscriptItem.User::class, TranscriptItem.Text::class), kinds("C-2"))
        assertEquals("Refactor checkout", (t.getValue(orchestratorKey("C-1"))[0] as TranscriptItem.User).text)
        assertEquals("Add feature x", (t.getValue(orchestratorKey("C-2"))[0] as TranscriptItem.User).text)
    }

    @Test
    fun putsALiveMessageIntoTheChatItIsForAndMarksANotificationForAnotherChatUnread() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        push(core, userMessage(5, 3, "more", "C-1"), inbox(6, 4, "C-1", InboxKind.HelpRaised, "stuck"))
        val t = s.view().transcripts
        assertEquals(
            listOf(TranscriptItem.User::class, TranscriptItem.Text::class, TranscriptItem.User::class, TranscriptItem.Notice::class),
            t.getValue(orchestratorKey("C-1")).map { it::class },
        )
        assertEquals(2, t.getValue(orchestratorKey("C-2")).size)
        assertEquals(setOf("C-1"), s.view().unread)
        s.selectChat("C-1")
        runCurrent()
        assertEquals(emptySet(), s.view().unread)
    }

    @Test
    fun stampsTheLastUseOfAChatWhenItsOrchestratorIsPrompted() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        val event = prompted(5, orchestratorKey("C-1"))
        push(core, event)
        assertEquals(event.tsMs, s.view().chats.find { it.id == "C-1" }?.lastUsedMs)
        assertEquals(CHAT_INFOS[0].lastUsedMs, s.view().chats.find { it.id == "C-2" }?.lastUsedMs)
    }

    // ---- commands ----

    @Test
    fun sendsAndCancelsOnTheSelectedChat() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertTrue(s.sendMessage("one"))
        s.selectChat("C-1")
        runCurrent()
        assertTrue(s.sendMessage("two"))
        s.cancelTurn()
        assertEquals(
            listOf(
                ApiCommand.SendUserMessage("repo", "C-2", "one"),
                ApiCommand.SendUserMessage("repo", "C-1", "two"),
                ApiCommand.CancelOrchestratorTurn("repo", "C-1"),
            ),
            core.calls.filter { it !is ApiCommand.OpenChat }.takeLast(3),
        )
    }

    @Test
    fun createsAChatInAWorktreeOrInABranchsAndSelectsIt() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertTrue(s.createChat(worktree = MAIN_WT))
        assertEquals("C-3", s.view().selectedChat)
        assertEquals(MAIN_WT, s.view().chats.find { it.id == "C-3" }?.worktree)
        assertTrue(s.createChat(branch = "feat/x"))
        assertEquals(
            listOf(ApiCommand.CreateChat("repo", worktree = MAIN_WT), ApiCommand.CreateChat("repo", branch = "feat/x")),
            core.callsOf<ApiCommand.CreateChat>(),
        )
    }

    @Test
    fun rereadsGitBeforeSelectingAChatMadeInABranchsNewWorktree() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        val reads = core.callsOf<ApiCommand.GetGitOverview>().size
        assertTrue(s.createChat(branch = "feat/new"))
        // チャットを選ぶ前に git を読み直している: 「見つからない作業ツリー」が一瞬出ない
        val calls = core.calls
        assertTrue(calls.indexOfLast { it is ApiCommand.GetGitOverview } > calls.indexOfLast { it is ApiCommand.CreateChat })
        assertEquals(reads + 1, core.callsOf<ApiCommand.GetGitOverview>().size)
        assertTrue(s.state.value.git?.overview?.worktrees.orEmpty().any { it.branch == "feat/new" })
        assertEquals("C-3", s.view().selectedChat)
    }

    @Test
    fun showsTheErrorAndKeepsTheSelectionWhenAChatCannotBeCreated() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        core.fail(ApiCommand.CreateChat::class, ApiErrorCode.NotFound, "branch gone: feat/x")
        assertFalse(s.createChat(branch = "feat/x"))
        assertTrue(s.state.value.errors[0].message.contains("branch gone"))
        assertEquals("C-2", s.view().selectedChat)
    }

    @Test
    fun createsABranchFromAStartPointOpensItsFirstChatAndRereadsGit() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        s.createBranch("feat/new", "main")
        assertEquals(listOf(ApiCommand.CreateBranch("repo", "feat/new", "main")), core.callsOf<ApiCommand.CreateBranch>())
        assertEquals("C-3", s.view().selectedChat)
        assertTrue(s.state.value.git?.overview?.branches.orEmpty().any { it.name == "feat/new" })
    }

    @Test
    fun throwsTheCoresErrorSoTheFormCanShowIt() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        core.branchError = ApiError(ApiErrorCode.Conflict, "branch feat/x already exists")
        val e = assertFailsWith<ApiException> { s.createBranch("feat/x") }
        assertEquals("branch feat/x already exists", e.error.message)
        assertEquals("C-2", s.view().selectedChat)
    }

    // ---- renaming and deleting chats ----

    @Test
    fun renamesAChatThroughTheCoreAndShowsTheTitleAtOnce() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        s.renameChat("C-1", "  Checkout\nwork ")
        assertEquals(listOf(ApiCommand.RenameChat("repo", "C-1", "  Checkout\nwork ")), core.callsOf<ApiCommand.RenameChat>())
        assertEquals("Checkout work", s.view().chats.find { it.id == "C-1" }?.title)
        assertEquals("Add feature x", s.view().chats.find { it.id == "C-2" }?.title)
        // コアのイベントも同じことを言い、それ以上は変えない
        push(core, domain(5, DomainEvent.ChatTitled("C-1", "Checkout work")))
        assertEquals("Checkout work", s.view().chats.find { it.id == "C-1" }?.title)
        assertEquals("Checkout work", s.view().state?.chats?.find { it.id == "C-1" }?.title)
    }

    @Test
    fun throwsTheCoresErrorSoTheEditorCanShowItAndShowsNoAppError() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        core.fail(ApiCommand.RenameChat::class, ApiErrorCode.InvalidArgument, "the title must not be empty")
        val e = assertFailsWith<ApiException> { s.renameChat("C-1", " ") }
        assertEquals(ApiErrorCode.InvalidArgument, e.error.code)
        assertEquals("Refactor checkout", s.view().chats.find { it.id == "C-1" }?.title)
        assertEquals(emptyList(), s.state.value.errors)
    }

    @Test
    fun deletesAChatItLeavesTheListAndAnotherChatIsShownIfItWasOnScreen() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertEquals("C-2", s.view().selectedChat)
        s.deleteChat("C-2")
        runCurrent()
        assertEquals(listOf(ApiCommand.DeleteChat("repo", "C-2")), core.callsOf<ApiCommand.DeleteChat>())
        assertEquals(listOf("C-1"), s.view().chats.map { it.id })
        assertEquals("C-1", s.view().selectedChat)
        // 続くイベントにはもう何も残っていない
        push(core, domain(5, DomainEvent.ChatDeleted("C-2")))
        assertEquals(listOf("C-1"), s.view().chats.map { it.id })
        assertEquals(listOf("G-1"), s.view().state?.groups?.map { it.id })
        assertNull(s.view().transcripts[orchestratorKey("C-2")])
    }

    @Test
    fun forgetsADeletedChatsDraftAndKeepsItWhenTheCoreRefuses() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        s.drafts.set("repo", "C-2", "typed")
        s.drafts.set("repo", "C-1", "other")
        core.fail(ApiCommand.DeleteChat::class, ApiErrorCode.InvalidState, "chat C-2 still has group G-2")
        assertFailsWith<ApiException> { s.deleteChat("C-2") }
        assertEquals("typed", s.drafts.get("repo", "C-2"))
        core.failures.remove(ApiCommand.DeleteChat::class)
        s.deleteChat("C-2")
        assertEquals("", s.drafts.get("repo", "C-2"))
        assertEquals("other", s.drafts.get("repo", "C-1"))
    }

    @Test
    fun keepsDraftsWhenTheProjectIsOpenedAgain() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        s.drafts.set("repo", "C-1", "typed")
        openAndSettle(s)
        assertEquals("typed", s.drafts.get("repo", "C-1"))
    }

    @Test
    fun keepsTheChatAndThrowsTheRefusalWhenTheCoreWillNotDeleteIt() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        core.fail(ApiCommand.DeleteChat::class, ApiErrorCode.InvalidState, "chat C-2 still has group G-2")
        val e = assertFailsWith<ApiException> { s.deleteChat("C-2") }
        assertEquals(ApiErrorCode.InvalidState to "chat C-2 still has group G-2", e.error.code to e.error.message)
        assertEquals(listOf("C-2", "C-1"), s.view().chats.map { it.id })
        assertEquals("C-2", s.view().selectedChat)
    }

    // ---- pinging the core (前面の間だけ) ----

    private fun pings() = core.callsOf<ApiCommand.Ping>().size

    @Test
    fun pingsWhenTheConnectionOpensAndKeepsTheHostNameAndTheTimeOfTheAnswer() = runTest {
        val s = newStore(core, prefs, foreground = true)
        assertEquals(1, pings())
        assertEquals(HostView("test-host", currentTime), s.state.value.host)
    }

    @Test
    fun pingsEveryFewSecondsAndStopsWhileTheConnectionIsClosed() = runTest {
        val s = newStore(core, prefs, foreground = true)
        advanceTimeBy(6_001)
        assertEquals(3, pings())
        val answered = s.state.value.host.lastPingAt
        core.state.value = ConnectionState.WaitingToReconnect(1, 1_000, "gone")
        advanceTimeBy(9_000)
        assertEquals(3, pings())
        assertEquals(HostView("test-host", answered), s.state.value.host)
        // つながり直すとまた始まる
        core.state.value = ConnectionState.Connected(2)
        runCurrent()
        assertEquals(4, pings())
    }

    @Test
    fun countsAFailedPingAsMissedWithoutShowingAnErrorAndKeepsTheLastAnswer() = runTest {
        val s = newStore(core, prefs, foreground = true)
        val answered = s.state.value.host.lastPingAt
        core.fail(ApiCommand.Ping::class, ApiErrorCode.Internal, "boom")
        advanceTimeBy(6_001)
        assertEquals(answered, s.state.value.host.lastPingAt)
        assertEquals(emptyList(), s.state.value.errors)
    }

    @Test
    fun givesUpOnAPingNobodyAnswersAndAsksAgain() = runTest {
        val never = CompletableDeferred<Unit>()
        core.before = { cmd -> if (cmd is ApiCommand.Ping) never.await() }
        val s = newStore(core, prefs, foreground = true)
        advanceTimeBy(5_000 + 3_000 + 1)
        assertTrue(pings() >= 2)
        assertNull(s.state.value.host.lastPingAt)
    }

    @Test
    fun pollsOnlyInTheForeground() = runTest {
        // Kotlin だけ: 裏では ping と使用量を読まない。前面に戻ったら git も読み直す (PC のウィンドウのフォーカス)
        val s = newStore(core, prefs, foreground = true)
        openAndSettle(s)
        s.setForeground(false)
        runCurrent()
        val before = pings()
        val usage = core.callsOf<ApiCommand.GetUsage>().size
        advanceTimeBy(10 * 60_000)
        assertEquals(before, pings())
        assertEquals(usage, core.callsOf<ApiCommand.GetUsage>().size)
        val gits = core.callsOf<ApiCommand.GetGitOverview>().size
        s.setForeground(true)
        runCurrent()
        assertEquals(before + 1, pings())
        assertEquals(usage + 1, core.callsOf<ApiCommand.GetUsage>().size)
        assertEquals(gits + 1, core.callsOf<ApiCommand.GetGitOverview>().size)
    }
}
