package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentChoice
import io.github.tom039224.yhtye.shared.api.generated.AgentRole
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.Chat
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.GitWorktree
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.RoleSettings
import io.github.tom039224.yhtye.shared.api.generated.SettingsKind
import io.github.tom039224.yhtye.shared.client.ApiException
import io.github.tom039224.yhtye.shared.client.ConnectionState
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * PC の `src/store/multiClient.test.ts` の移植: 同じコアに他のクライアントがいるとき — アプリ全体のイベント
 * (projects_changed, settings_changed, resync)、他のクライアントが起動したチャット、表示中のチャットが他で
 * 削除された、まだ git を読んでいない作業ツリーに他で作られたチャット。Kotlin で `expected_revision` の conflict も。
 */
@OptIn(ExperimentalCoroutinesApi::class)
class MultiClientTest {
    private val core = FakeCore(PROJECT, chatSnapshot()).apply { log = CHAT_LOG }
    private val prefs = MemoryPrefs()
    private val chatLog = CHAT_LOG.size.toLong()

    // ---- app-wide events ----

    @Test
    fun rereadsTheProjectListWhenAnotherClientOpensAProject() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        core.others = listOf(ProjectInfo("other", "other", "/tmp/other", open = true, running = false, awake = false))
        val before = core.callsOf<ApiCommand.ListProjects>().size
        push(core, appWide(ApiEventBody.ProjectsChanged("other")))
        assertEquals(listOf(PROJECT.id, "other"), s.state.value.projects.map { it.id })
        assertEquals(before + 1, core.callsOf<ApiCommand.ListProjects>().size)
        // 開いているプロジェクトには畳み込まない
        assertEquals(chatLog, s.view().cursor)
    }

    @Test
    fun passesSettingsChangesToTheSettingsScreens() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        val seen = collected(s.settings.changes)
        push(
            core,
            appWide(ApiEventBody.SettingsChanged(SettingsKind.AgentSettings, "repo", 7)),
            appWide(ApiEventBody.SettingsChanged(SettingsKind.Secrets)),
        )
        assertEquals(
            listOf(SettingsChange(SettingsKind.AgentSettings, "repo", 7), SettingsChange(SettingsKind.Secrets, null, null)),
            seen,
        )
    }

    @Test
    fun catchesUpAfterAResyncTheLogTheProjectListGitTheChatOnScreenAndTheSettings() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        val seen = collected(s.settings.changes)
        // このクライアントには届かなかったイベント (捨てられた)
        core.log = CHAT_LOG + prompted(chatLog + 1, orchestratorKey("C-2"), "missed")
        core.calls.clear()
        push(core, appWide(ApiEventBody.Resync))
        assertEquals(chatLog + 1, s.view().cursor)
        assertEquals(chatLog, core.callsOf<ApiCommand.ListEvents>().first().afterSeq)
        assertEquals(1, core.callsOf<ApiCommand.ListProjects>().size)
        assertEquals(1, core.callsOf<ApiCommand.GetGitOverview>().size)
        assertEquals(listOf(ApiCommand.OpenChat("repo", "C-2")), core.callsOf<ApiCommand.OpenChat>())
        assertEquals(listOf(SettingsChange.EVERYTHING), seen)
    }

    @Test
    fun tellsTheSettingsScreensThatAnythingMayHaveChangedAfterAReconnect() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        val seen = collected(s.settings.changes)
        core.state.value = ConnectionState.WaitingToReconnect(1, 100, "gone")
        runCurrent()
        core.state.value = ConnectionState.Connected(2)
        runCurrent()
        assertEquals(listOf(SettingsChange.EVERYTHING), seen)
    }

    // ---- a chat starting ----

    @Test
    fun showsAChatAnotherClientIsStartingFromItsLiveEvent() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertEquals(emptySet(), s.view().resuming)
        push(core, ev(chatLog, ApiEventBody.SessionStarting(orchestratorKey("C-1")), live = true))
        assertEquals(setOf("C-1"), s.view().resuming)
        // その起動で終わる
        val started = sessionStarted(chatLog + 1, orchestratorKey("C-1"))
        core.log = CHAT_LOG + started
        push(core, started)
        assertEquals(emptySet(), s.view().resuming)
    }

    @Test
    fun showsTheChatsBeingStartedWhenTheProjectIsLoaded() = runTest {
        // 後からつないだクライアント
        core.snapshot = chatSnapshot().copy(starting = listOf(orchestratorKey("C-1"), "T-1/0"))
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertEquals(setOf("C-1"), s.view().resuming)
    }

    // ---- the chat on screen deleted by another client ----

    @Test
    fun switchesToAnotherChatRemembersItOpensItAndDropsTheDeletedChatsDraft() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertEquals("C-2", s.view().selectedChat)
        s.drafts.set(PROJECT.id, "C-2", "half-written")
        core.calls.clear()
        val deleted = domain(chatLog + 1, DomainEvent.ChatDeleted("C-2"))
        core.log = CHAT_LOG + deleted
        push(core, deleted)
        assertEquals("C-1", s.view().selectedChat)
        assertEquals(listOf("C-1"), s.view().chats.map { it.id })
        assertEquals("C-1", prefs.get(chatPrefKey(PROJECT.id)))
        assertEquals("", s.drafts.get(PROJECT.id, "C-2"))
        assertEquals(listOf(ApiCommand.OpenChat("repo", "C-1")), core.callsOf<ApiCommand.OpenChat>())
    }

    @Test
    fun leavesTheSelectionAloneWhenAnotherChatIsDeleted() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        s.drafts.set(PROJECT.id, "C-1", "draft")
        core.calls.clear()
        val deleted = domain(chatLog + 1, DomainEvent.ChatDeleted("C-1"))
        core.log = CHAT_LOG + deleted
        push(core, deleted)
        assertEquals("C-2", s.view().selectedChat)
        assertEquals("", s.drafts.get(PROJECT.id, "C-1"))
        assertEquals(emptyList(), core.callsOf<ApiCommand.OpenChat>())
    }

    // ---- a chat made by another client ----

    @Test
    fun isShownRememberedAndOpenedWhenTheProjectHadNone() = runTest {
        core.snapshot = chatSnapshot(emptyList(), chatState().copy(chats = emptyList(), groups = emptyList(), tasks = emptyList())).copy(seq = 0)
        core.log = emptyList()
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertNull(s.view().selectedChat)
        val chat = Chat("C-1", "/repo", null)
        val created = domain(1, DomainEvent.ChatCreated(chat))
        core.log = listOf(created)
        core.snapshot = core.snapshot.copy(chats = listOf(ChatInfo(chat.id, chat.worktree, null, 1, 1)))
        push(core, created)
        assertEquals("C-1", s.view().selectedChat)
        assertEquals("C-1", prefs.get(chatPrefKey(PROJECT.id)))
        assertEquals(listOf(ApiCommand.OpenChat("repo", "C-1")), core.callsOf<ApiCommand.OpenChat>())
        // 2 つ目は画面を取らない
        push(core, domain(2, DomainEvent.ChatCreated(chat.copy(id = "C-2"))))
        assertEquals("C-1", s.view().selectedChat)
    }

    @Test
    fun readsGitAtOnceAndShowsTheChatsPlaceAsUnknownNotMissingUntilThen() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertEquals(false, s.state.value.git?.loading)
        val wt = "/data/worktrees/repo/branches/new"
        val chat = Chat("C-3", wt, null)
        // git の読み取りの応答は、テストが離すまで止める
        val held = CompletableDeferred<Unit>()
        core.before = { cmd -> if (cmd is ApiCommand.GetGitOverview) held.await() }
        val reads = core.callsOf<ApiCommand.GetGitOverview>().size
        val created = domain(chatLog + 1, DomainEvent.ChatCreated(chat))
        core.log = CHAT_LOG + created
        push(core, created)
        // 間を置かずにすぐ読む (遅延の読み直しではない)
        assertEquals(reads + 1, core.callsOf<ApiCommand.GetGitOverview>().size)
        val git = { s.state.value.git }
        val info = ChatInfo(chat.id, wt, null, 1, 1)
        assertEquals(listOf("C-3"), git()?.awaiting)
        assertEquals(Place.Unknown, placeOf(info, git()?.overview, "C-3" in git()?.awaiting.orEmpty()))
        // git は新しい作業ツリーを返す
        core.git = core.git.copy(worktrees = core.git.worktrees + GitWorktree(wt, "new", null, isMain = false, missing = false))
        held.complete(Unit)
        runCurrent()
        assertEquals(false, git()?.loading)
        assertEquals(emptyList(), git()?.awaiting)
        assertEquals(Place.Branch("new"), placeOf(info, git()?.overview, false))
        // 読んだ後も一覧に無い作業ツリーのチャットは「見つからない」
        assertEquals(Place.Missing, placeOf(info.copy(worktree = "/gone"), git()?.overview, false))
    }

    @Test
    fun doesNotReadGitAgainForAChatInAWorktreeGitListsAlready() = runTest {
        val s = newStore(core, prefs)
        openAndSettle(s)
        assertEquals(false, s.state.value.git?.loading)
        val reads = core.callsOf<ApiCommand.GetGitOverview>().size
        push(core, domain(chatLog + 1, DomainEvent.ChatCreated(Chat("C-3", MAIN_WT, null))))
        assertTrue(s.view().chats.any { it.id == "C-3" })
        assertEquals(reads, core.callsOf<ApiCommand.GetGitOverview>().size)
        assertEquals(emptyList(), s.state.value.git?.awaiting)
    }

    @Test
    fun readsGitOnceForAChatMadeWhileTheProjectLoadsThoughTheEventIsSeenAgainFromTheBuffer() = runTest {
        val wt = "/data/worktrees/repo/branches/new"
        val created = domain(chatLog + 1, DomainEvent.ChatCreated(Chat("C-3", wt, null)))
        core.log = CHAT_LOG + created
        val s = newStore(core, prefs)
        // スナップショットはイベントが来るまで止める: イベントは溜められ、読み込みの後で流し直される
        val held = CompletableDeferred<Unit>()
        core.before = { cmd -> if (cmd is ApiCommand.GetSnapshot) held.await() }
        val opening = launch { s.openProject(PROJECT.path) }
        runCurrent()
        assertEquals(false, s.state.value.git?.loading)
        // コアはチャットを知らせる前に作業ツリーを作っている
        core.git = core.git.copy(worktrees = core.git.worktrees + GitWorktree(wt, "new", null, isMain = false, missing = false))
        push(core, created)
        held.complete(Unit)
        runCurrent()
        opening.join()
        assertEquals(LoadPhase.Ready, s.view().phase)
        assertTrue(s.view().chats.any { it.id == "C-3" })
        // 読み込みが始めた読み取りと、イベントがすぐ頼んだもの。流し直しのためには読まない
        assertEquals(2, core.callsOf<ApiCommand.GetGitOverview>().size)
        assertEquals(emptyList(), s.state.value.git?.awaiting)
    }

    // ---- expected_revision と conflict (Kotlin で足した: PC は SettingsSync.test.tsx の画面で確かめている) ----

    private val settings = RoleSettings(emptyList(), AgentChoice("claude-code", "haiku"))

    @Test
    fun savesTheAgentSettingsWhenNobodyChangedThemSinceTheyWereRead() = runTest {
        val s = newStore(core, prefs)
        val shown = s.settings.getAgentSettings(null)
        val saved = assertIs<AgentSettingsSave.Saved>(s.settings.setAgentSettings(null, AgentRole.Implementer, settings, shown.revision))
        assertEquals(shown.revision + 1, saved.settings.revision)
        val sent = core.callsOf<ApiCommand.SetAgentSettings>().single()
        assertEquals(shown.revision, sent.expectedRevision)
    }

    @Test
    fun doesNotOverwriteSettingsAnotherClientChangedAndReadsThemAgain() = runTest {
        val s = newStore(core, prefs)
        val shown = s.settings.getAgentSettings("repo")
        // 他のクライアントが保存した
        core.agentRevision += 1
        val result = s.settings.setAgentSettings("repo", AgentRole.Implementer, settings, shown.revision)
        val conflict = assertIs<AgentSettingsSave.Conflict>(result)
        assertEquals(core.agentRevision, conflict.latest.revision)
        assertEquals(0, core.agentSettingsSaves)
        // 読み直した版で保存し直せる
        assertIs<AgentSettingsSave.Saved>(s.settings.setAgentSettings("repo", AgentRole.Implementer, settings, conflict.latest.revision))
        assertEquals(1, core.agentSettingsSaves)
    }

    @Test
    fun throwsOtherSettingsFailuresForTheScreenToShow() = runTest {
        val s = newStore(core, prefs)
        core.fail(ApiCommand.SetAgentSettings::class, ApiErrorCode.InvalidArgument, "unknown harness")
        val e = assertFailsWith<ApiException> { s.settings.setAgentSettings(null, AgentRole.Implementer, settings, null) }
        assertEquals("unknown harness", e.error.message)
        assertEquals(emptyList(), s.state.value.errors)
    }
}
