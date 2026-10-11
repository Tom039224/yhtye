package io.github.tom039224.yhtye.ui.conversation

import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.sync.ContextUsage
import org.junit.Test
import kotlin.test.assertEquals

class ConversationModelTest {
    @Test
    fun `compact is offered only for a running idle orchestrator`() {
        assertEquals(CompactState.Ready, compactState(null, SessionStatus.Live, turnRunning = false, compacting = false))
        assertEquals(CompactState.Compacting, compactState("x", SessionStatus.Live, turnRunning = true, compacting = true))
        assertEquals(CompactState.Disabled("コアに接続していません"), compactState("コアに接続していません", SessionStatus.Live, false, false))
        assertEquals(
            CompactState.Disabled("オーケストレータが起動していないため圧縮できません"),
            compactState(null, SessionStatus.Suspended, turnRunning = false, compacting = false),
        )
        assertEquals(
            CompactState.Disabled("作業中は圧縮できません (ターン終了後に押せます)"),
            compactState(null, SessionStatus.Live, turnRunning = true, compacting = false),
        )
    }

    @Test
    fun `the session badge has the PC's words`() {
        assertEquals("starting", sessionBadge(SessionStatus.Live, running = false, starting = true).label)
        assertEquals("not started", sessionBadge(null, running = false, starting = false).label)
        assertEquals(SessionBadge.Working, sessionBadge(SessionStatus.Live, running = true, starting = false))
        assertEquals(SessionBadge.Ready, sessionBadge(SessionStatus.Live, running = false, starting = false))
        assertEquals(SessionBadge.Idle("suspended"), sessionBadge(SessionStatus.Suspended, running = false, starting = false))
        assertEquals(SessionBadge.Bad("stopped"), sessionBadge(SessionStatus.Stopped, running = false, starting = false))
    }

    @Test
    fun `the context meter reads like the PC's`() {
        assertEquals("—", meterText(null))
        assertEquals("42% · 84k / 200k", meterText(ContextUsage(84_000.0, 200_000.0, null)))
        assertEquals("950 / —", meterText(ContextUsage(950.0, 0.0, null)))
        assertEquals("1.2M", tokens(1_200_000.0))
        assertEquals("1M", tokens(1_000_000.0))
    }

    @Test
    fun `mentions go first, one line each, and are not added twice`() {
        val list = addMention(addMention(emptyList(), Mention("T-1", "a")), Mention("T-1", "a"))
        assertEquals(1, list.size)
        assertEquals("@T-1 a\n@T-2 b\n\nhi", withMentions("hi", list + Mention("T-2", "b")))
        assertEquals("hi", withMentions("hi", emptyList()))
        assertEquals(emptyList(), removeMention(list, "T-1"))
    }
}
