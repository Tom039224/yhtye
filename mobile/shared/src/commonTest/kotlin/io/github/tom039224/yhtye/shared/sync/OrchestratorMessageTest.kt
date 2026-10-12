package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull

/**
 * PC の `src/store/orchestratorMessage.test.ts` と `transcript.test.tsx` の移植 (事例も文字列も同じ。
 * 文字列は `prompts::orchestrator_message_prompt` が作るもの)。
 */
class OrchestratorMessageTest {
    private val header = "[yhtye:orchestrator_message]"
    private val interrupted = "(Your previous turn was interrupted by Yhtye to deliver this message; your work so far is kept.)"
    private val note =
        "(This message is from the orchestrator while you are working on the current step. Take it into account and continue the step; when finished, call report_step_done (or help) as usual.)"
    private val step = "[yhtye:step] task=T-1 kind=code step=1/2 step_kind=implement\nInstruction:\nAppend hello"

    private fun plain(text: String) = OrchestratorMessage(text, interrupted = false)

    @Test
    fun takesTheTextBetweenTheHeaderAndTheNoteForTheAgent() {
        assertEquals(plain("use tabs"), parseOrchestratorMessage("$header\nuse tabs\n\n$note"))
    }

    @Test
    fun flagsAMessageResentAfterAnInterruptAndDropsItsLeadInLine() {
        assertEquals(
            OrchestratorMessage("use tabs", interrupted = true),
            parseOrchestratorMessage("$header\n$interrupted\nuse tabs\n\n$note"),
        )
    }

    @Test
    fun keepsAMultiLineMarkdownTextWholeBlankLinesIncluded() {
        val text = "## 方針\n\n- `v3` へ寄せる\n- テストも直す\n\n```ts\nconst a = 1;\n```"
        assertEquals(plain(text), parseOrchestratorMessage("$header\n$text\n\n$note"))
    }

    @Test
    fun dropsOnlyTheClosingNoteWhenTheTextQuotesIt() {
        val text = "次の注記は無視して:\n\n$note\n\nそのまま続ける"
        assertEquals(text, parseOrchestratorMessage("$header\n$text\n\n$note")?.text)
    }

    @Test
    fun doesNotTakeTheInterruptLineFromTheMiddleOfTheText() {
        val text = "a\n$interrupted"
        assertEquals(plain(text), parseOrchestratorMessage("$header\n$text\n\n$note"))
    }

    @Test
    fun stillReadsAMessageWhoseNoteIsMissingAndOneWithWindowsLineEnds() {
        assertEquals(plain("use tabs"), parseOrchestratorMessage("$header\nuse tabs"))
        assertEquals(
            OrchestratorMessage("use tabs", interrupted = true),
            parseOrchestratorMessage("$header\r\n$interrupted\r\nuse tabs\r\n\r\n$note"),
        )
    }

    @Test
    fun toleratesARewordedNoteAfterItsOpeningWords() {
        assertEquals(
            "use tabs",
            parseOrchestratorMessage("$header\nuse tabs\n\n(This message is from the orchestrator. Carry on.)")?.text,
        )
    }

    @Test
    fun isNullForAnyOtherPrompt() {
        assertNull(parseOrchestratorMessage(""))
        assertNull(parseOrchestratorMessage("use tabs"))
        assertNull(parseOrchestratorMessage(step))
        // 見出しは 1 行目に単独で
        assertNull(parseOrchestratorMessage("\n$header\nuse tabs\n\n$note"))
        assertNull(parseOrchestratorMessage("quote: $header\nuse tabs"))
        assertNull(parseOrchestratorMessage("$header use tabs"))
        assertNull(parseOrchestratorMessage("${header}x\nuse tabs"))
    }

    @Test
    fun isNullWhenThereIsNoTextOfTheOrchestratorsToShow() {
        assertNull(parseOrchestratorMessage(header))
        assertNull(parseOrchestratorMessage("$header\n\n$note"))
        assertNull(parseOrchestratorMessage("$header\n$interrupted\n\n$note"))
        assertNull(parseOrchestratorMessage("$header\n$note"))
    }

    private fun fold(vararg texts: String): List<TranscriptItem> {
        val events = texts.mapIndexed { i, text -> ev(i + 1L, ApiEventBody.Prompted("T-1/implementer", text)) }
        return events.fold(emptyMap<String, List<TranscriptItem>>()) { t, e -> applyTranscriptEvent(t, e) }
            .getValue("T-1/implementer")
    }

    @Test
    fun isItsOwnTranscriptItemAndAnyOtherPromptStaysAPrompt() {
        val items = fold(step, "$header\n**タブ**を使う\n\n$note", "$header\n$interrupted\n\n$note")
        assertEquals(
            listOf("Prompt", "OrchestratorMessage", "Prompt"),
            items.map { it::class.simpleName },
        )
        val message = assertIs<TranscriptItem.OrchestratorMessage>(items[1])
        assertEquals("**タブ**を使う" to false, message.text to message.interrupted)
        assertEquals(step, assertIs<TranscriptItem.Prompt>(items[0]).text)
    }

    @Test
    fun carriesTheInterruptFlagOntoTheItem() {
        val message = assertIs<TranscriptItem.OrchestratorMessage>(fold("$header\n$interrupted\nタブを使う\n\n$note").single())
        assertEquals("タブを使う" to true, message.text to message.interrupted)
    }
}
