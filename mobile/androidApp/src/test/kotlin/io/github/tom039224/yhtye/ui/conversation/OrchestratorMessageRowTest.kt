package io.github.tom039224.yhtye.ui.conversation

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Column
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import io.github.tom039224.yhtye.YhtyeTheme
import io.github.tom039224.yhtye.shared.sync.TranscriptItem
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/**
 * タスクのエージェントの会話の、オーケストレータの `message_task` のメッセージ (PC の `transcript.test.tsx` の
 * 表示の確認と同じ事例)。読み方は shared の OrchestratorMessageTest。
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w400dp-h800dp")
class OrchestratorMessageRowTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private fun show(vararg items: TranscriptItem) {
        rule.setContent {
            YhtyeTheme(darkTheme = true) {
                Column { items.forEach { TranscriptRow(it, emptySet(), null) } }
            }
        }
    }

    private fun count(tag: String) = rule.onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().size

    @Test
    fun showsTheTextLabelledAsTheOrchestratorsAsMarkdownWithoutTheNoteForTheAgent() {
        show(TranscriptItem.OrchestratorMessage(1, 1, "**タブ**を使う\n\n- 一つ目", interrupted = false))
        rule.onNodeWithTag("orchestrator-message").assertExists()
        rule.onNodeWithText("オーケストレータから").assertExists()
        // 強調は記号ごと文字にならず、本文として出る
        rule.onNodeWithText("タブを使う", substring = true).assertExists()
        assertEquals(0, rule.onAllNodes(hasText("**", substring = true)).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodes(hasText("This message is from the orchestrator", substring = true)).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodes(hasText("[yhtye:orchestrator_message]", substring = true)).fetchSemanticsNodes().size)
        assertEquals(0, count("fold"))
        assertEquals(0, count("orchestrator-message-interrupted"))
    }

    @Test
    fun saysWhenItWasSentAgainAfterAnInterrupt() {
        show(TranscriptItem.OrchestratorMessage(1, 1, "タブを使う", interrupted = true))
        rule.onNodeWithText("割り込みで再送").assertExists()
        rule.onNodeWithText("タブを使う").assertExists()
    }

    @Test
    fun leavesAStepPromptInItsFoldedForm() {
        val step = "[yhtye:step] task=T-1 kind=code step=1/2 step_kind=implement\nInstruction:\nAppend hello"
        show(TranscriptItem.Prompt(1, 1, step))
        assertEquals(0, count("orchestrator-message"))
        assertEquals(1, count("fold"))
        assertEquals(0, count("fold-body"))
    }
}
