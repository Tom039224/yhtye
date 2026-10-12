package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.Chat
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.Role
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

/** PC の `src/store/sessions.test.ts`・`chats.test.ts`・`drafts.test.ts` の移植。 */
class ChatsAndSessionsTest {
    // ---- sessions.test.ts: 復元できなかったセッション ----

    private val key = "T-1/implementer"

    private fun started(acp: String, lost: String? = null) = ApiEventBody.SessionStarted(
        session = key,
        role = Role.Implementer,
        task = "T-1",
        pid = 1,
        acpSessionId = acp,
        resumed = false,
        agent = null,
        lostAcpSessionId = lost,
    )

    @Test
    fun recordsTheLostAcpSessionNextToTheNewOne() {
        val first = applySessionEvent(emptyList(), started("acp-1"))
        assertNull(first[0].lostAcpSessionId)
        val replaced = applySessionEvent(first, started("acp-2", "acp-1"))
        assertEquals(1, replaced.size)
        assertEquals("acp-2" to "acp-1", replaced[0].acpSessionId to replaced[0].lostAcpSessionId)
    }

    @Test
    fun keepsItOverALaterStartThatLostNothingUntilAnotherIsLost() {
        var sessions = applySessionEvent(emptyList(), started("acp-2", "acp-1"))
        sessions = applySessionEvent(sessions, started("acp-2"))
        assertEquals("acp-1", sessions[0].lostAcpSessionId)
        sessions = applySessionEvent(sessions, started("acp-3", "acp-2"))
        assertEquals("acp-3" to "acp-2", sessions[0].acpSessionId to sessions[0].lostAcpSessionId)
    }

    // ---- chats.test.ts ----

    @Test
    fun addsACreatedChatOnceAndTakesTheTimesFromTheEvent() {
        val created = domain(9, DomainEvent.ChatCreated(Chat("C-3", "/wt/dev", null)))
        val once = applyChatEvent(CHAT_INFOS, created)
        val last = once.last()
        assertEquals(listOf("C-3", "/wt/dev"), listOf(last.id, last.worktree))
        assertEquals(created.tsMs to created.tsMs, last.createdMs to last.lastUsedMs)
        assertEquals(3, applyChatEvent(once, created).size)
    }

    @Test
    fun titlesAChatAndStampsItsLastUseWhenItsOrchestratorIsPrompted() {
        val titled = domain(9, DomainEvent.ChatTitled("C-1", "Hello"))
        assertEquals("Hello", applyChatEvent(CHAT_INFOS, titled).find { it.id == "C-1" }?.title)
        val prompt = prompted(10, orchestratorKey("C-1"), "x")
        assertEquals(prompt.tsMs, applyChatEvent(CHAT_INFOS, prompt).find { it.id == "C-1" }?.lastUsedMs)
        // サブエージェントのプロンプトはチャットの使用ではない
        assertSame(CHAT_INFOS, applyChatEvent(CHAT_INFOS, prompted(11, "T-1/implementer", "x")))
    }

    @Test
    fun dropsADeletedChatFromTheListOnce() {
        val deleted = domain(9, DomainEvent.ChatDeleted("C-2"))
        val list = applyChatEvent(CHAT_INFOS, deleted)
        assertEquals(listOf("C-1"), list.map { it.id })
        assertEquals(list, applyChatEvent(list, deleted))
    }

    @Test
    fun picksTheRememberedChatIfItExistsElseTheMostRecentlyUsed() {
        assertEquals("C-2", initialChat(CHAT_INFOS, null))
        assertEquals("C-1", initialChat(CHAT_INFOS, "C-1"))
        assertEquals("C-2", initialChat(CHAT_INFOS, "C-9"))
        assertNull(initialChat(emptyList(), "C-1"))
    }

    @Test
    fun scopesGroupsTasksAndTheInboxToOneChat() {
        val s = scopeState(chatState(), "C-2")
        assertEquals(listOf("G-2"), s.groups.map { it.id })
        assertEquals(listOf("T-2"), s.tasks.map { it.id })
        assertEquals(emptyList(), scopeState(chatState(), null).groups)
    }

    private fun args(vararg pairs: Pair<String, String>): JsonObject = buildJsonObject { pairs.forEach { (k, v) -> put(k, v) } }

    @Test
    fun findsTheChatOfAUsersCancelCallFromItsGroupOrTask() {
        val state = chatState()
        assertEquals("C-2", chatOfUserCall(state, args("group_id" to "G-2", "reason" to "r")))
        assertEquals("C-1", chatOfUserCall(state, args("task_id" to "T-1", "reason" to "r")))
        assertNull(chatOfUserCall(state, args("task_id" to "T-9")))
        assertNull(chatOfUserCall(null, args("task_id" to "T-1")))
        assertNull(chatOfUserCall(state, JsonObject(emptyMap())))
    }

    @Test
    fun recognisesYhtyesInternalBranches() {
        assertTrue(isInternalBranch("yhtye/G-1"))
        assertTrue(!isInternalBranch("feat/yhtye"))
    }

    // ---- drafts.test.ts (下書きは端末の設定に残す: PrefsDrafts) ----

    @Test
    fun isEmptyForAChatThatHasNoDraft() {
        assertEquals("", PrefsDrafts(MemoryPrefs()).get("repo", "C-1"))
    }

    @Test
    fun keepsADraftPerChat() {
        val drafts = PrefsDrafts(MemoryPrefs())
        drafts.set("repo", "C-1", "first")
        drafts.set("repo", "C-2", "second")
        assertEquals("first", drafts.get("repo", "C-1"))
        assertEquals("second", drafts.get("repo", "C-2"))
    }

    @Test
    fun keepsTheSameChatIdOfTwoProjectsApart() {
        val drafts = PrefsDrafts(MemoryPrefs())
        drafts.set("repo", "C-1", "in repo")
        drafts.set("other", "C-1", "in other")
        assertEquals("in repo", drafts.get("repo", "C-1"))
        assertEquals("in other", drafts.get("other", "C-1"))
    }

    @Test
    fun doesNotMixUpIdsThatOnlyJoinIntoTheSameString() {
        val drafts = PrefsDrafts(MemoryPrefs())
        drafts.set("a/b", "c", "x")
        assertEquals("", drafts.get("a", "b/c"))
    }

    @Test
    fun replacesADraftAndForgetsItWhenSetToAnEmptyText() {
        val prefs = MemoryPrefs()
        val drafts = PrefsDrafts(prefs)
        drafts.set("repo", "C-1", "old")
        drafts.set("repo", "C-1", "new")
        assertEquals("new", drafts.get("repo", "C-1"))
        drafts.set("repo", "C-1", "")
        assertEquals("", drafts.get("repo", "C-1"))
        // 空の下書きは保存先にも残らない
        assertNull(prefs.get("yhtye.draft.[\"repo\",\"C-1\"]"))
    }

    @Test
    fun doesNotShareDraftsBetweenStores() {
        val a = PrefsDrafts(MemoryPrefs())
        a.set("repo", "C-1", "mine")
        assertEquals("", PrefsDrafts(MemoryPrefs()).get("repo", "C-1"))
    }

    @Test
    fun keepsDraftsInTheGivenPrefs() {
        // Kotlin だけ: 同じ保存先なら (アプリを起動し直しても) 残る
        val prefs = MemoryPrefs()
        PrefsDrafts(prefs).set("repo", "C-1", "kept")
        assertEquals("kept", PrefsDrafts(prefs).get("repo", "C-1"))
    }
}
