package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentChoice
import io.github.tom039224.yhtye.shared.api.generated.AgentEvent
import io.github.tom039224.yhtye.shared.api.generated.AgentOutput
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.Role
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject
import kotlin.test.Test
import kotlin.test.assertEquals

/**
 * PC の `src/store/transcript.test.tsx` の移植 (画面の描画の確認は除く。描画は次のタスクの画面で)。
 * OpenCode はコードモードの `execute` から MCP のツールを呼ぶので、呼んだツールを `rawInput.code` から読む。
 */
class TranscriptTest {
    private val s = "T-1/implementer"

    private fun toolEvent(seq: Long, update: Boolean, body: JsonElement): ApiEvent {
        val out = if (update) AgentOutput.ToolCallUpdate(body) else AgentOutput.ToolCall(body)
        return ev(seq, ApiEventBody.Agent(s, AgentEvent.Output(out)))
    }

    private fun fold(events: List<ApiEvent>): Transcripts =
        events.fold(emptyMap()) { t, e -> applyTranscriptEvent(t, e) }

    @Test
    fun listsTheMcpToolsTheCodeCallsOnceEachInOrder() {
        val code = """const s = await tools.yhtye.get_status({});
await tools.yhtye.report_step_done({ result: "ok" });
await tools.yhtye.get_status({}); tools . nope(); tools.other_server.x ( 1 )"""
        assertEquals(listOf("yhtye.get_status", "yhtye.report_step_done", "other_server.x"), codeToolCalls(code))
        assertEquals(emptyList(), codeToolCalls(null))
        assertEquals(emptyList(), codeToolCalls("ls -la"))
    }

    @Test
    fun showsTheToolsAnOpenCodeExecuteCallRan() {
        val t = fold(
            listOf(
                toolEvent(
                    1,
                    false,
                    buildJsonObject {
                        put("toolCallId", "c1"); put("title", "execute"); put("kind", "other"); put("status", "pending")
                        putJsonObject("rawInput") {}
                    },
                ),
                toolEvent(
                    2,
                    true,
                    buildJsonObject {
                        put("toolCallId", "c1"); put("status", "in_progress")
                        putJsonObject("rawInput") { put("code", "await tools.yhtye.report_step_done({ result: \"done\" })") }
                    },
                ),
                toolEvent(3, true, buildJsonObject { put("toolCallId", "c1"); put("status", "completed") }),
                toolEvent(
                    4,
                    false,
                    buildJsonObject {
                        put("toolCallId", "c2"); put("title", "bash"); put("kind", "execute")
                        putJsonObject("rawInput") { put("command", "ls") }
                    },
                ),
            ),
        )
        val items = t.getValue(s).map { it as TranscriptItem.Tool }
        assertEquals(2, items.size)
        assertEquals(listOf("execute", "completed"), listOf(items[0].title, items[0].status))
        assertEquals(listOf("yhtye.report_step_done"), items[0].calls)
        assertEquals("bash" to emptyList<String>(), items[1].title to items[1].calls)
    }

    private fun lifecycle(t: Transcripts): List<Pair<String, Boolean>> =
        t.getValue(s).map { (it as TranscriptItem.Lifecycle).let { l -> l.text to l.error } }

    @Test
    fun addsAnErrorLineNamingWhatWasAskedForAndWhatRanWhenAHarnessWasReplaced() {
        val started = { replaced: Boolean ->
            ApiEventBody.SessionStarted(
                session = s, role = Role.Implementer, task = "T-1", pid = 1, acpSessionId = "x", resumed = false,
                agent = AgentChoice("claude-code", "haiku"),
                replaced = if (replaced) AgentChoice("opencode", "opencode/free") else null,
            )
        }
        val t = fold(listOf(ev(1, started(true)), ev(2, started(false))))
        assertEquals(
            listOf(
                "session started" to false,
                "opencode/opencode/free is not available (not installed?); started claude-code/haiku instead" to true,
                "session started" to false,
            ),
            lifecycle(t),
        )
    }

    @Test
    fun namesTheLostAcpSessionInAnErrorLineInsteadOfAPlainStart() {
        val started = { lost: String? ->
            ApiEventBody.SessionStarted(s, Role.Implementer, "T-1", 1, "new-id", resumed = false, agent = null, lostAcpSessionId = lost)
        }
        val t = fold(listOf(ev(1, started("old-id")), ev(2, started(null))))
        assertEquals(
            listOf("could not restore the previous session (old-id); started a new session" to true, "session started" to false),
            lifecycle(t),
        )
    }

    @Test
    fun addsNothingToTheConversationWhenAGroupIsMergedIntoAnotherBranch() {
        assertEquals(emptyMap(), fold(listOf(domain(1, DomainEvent.GroupBaseChanged("G-1", "feat/x", "feat/y")))))
    }

    @Test
    fun putsAToolCallStartedInAnOlderPageOnceAtItsStartWithTheNewerStatus() {
        // Kotlin で足した: 古い履歴を前に足すとき (PC は app.test.ts の遅延読み込みで間接に確かめている)
        val older = fold(listOf(toolEvent(1, false, buildJsonObject { put("toolCallId", "c1"); put("title", "Read") })))
        val newer = fold(listOf(toolEvent(2, true, buildJsonObject { put("toolCallId", "c1"); put("status", "completed") }), agentText(3, s, "done")))
        val merged = prependTranscripts(older, newer).getValue(s)
        assertEquals(2, merged.size)
        val tool = merged[0] as TranscriptItem.Tool
        assertEquals(Triple(1L, "Read", "completed"), Triple(tool.seq, tool.title, tool.status))
        assertEquals(3L, merged[1].seq)
    }
}
