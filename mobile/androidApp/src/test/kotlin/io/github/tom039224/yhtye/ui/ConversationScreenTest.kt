package io.github.tom039224.yhtye.ui

import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import io.github.tom039224.yhtye.YhtyeTheme
import io.github.tom039224.yhtye.ui.panels.MainView
import io.github.tom039224.yhtye.shared.api.generated.AgentEvent
import io.github.tom039224.yhtye.shared.api.generated.AgentOutput
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.Role
import io.github.tom039224.yhtye.shared.api.generated.SessionRecord
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.api.generated.Snapshot
import io.github.tom039224.yhtye.shared.api.generated.StepStatus
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.LoadPhase
import io.github.tom039224.yhtye.shared.sync.orchestratorKey
import io.github.tom039224.yhtye.ui.conversation.NO_CHAT_HINT
import io.github.tom039224.yhtye.ui.conversation.pngBytes
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * 中央の会話: 本物の同期層 ([AppStore]) を偽のコアの上で動かし、画面の操作がコアへの要求になることを確かめる
 * (楽観的に変えない。表示はコアの状態とイベントから)。
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "w400dp-h800dp")
class ConversationScreenTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val recording = readRecording()
    /** 同期層を動かすスコープ。テストの規則が Main を差し替えた後に作る (`waitUntil` が進める)。 */
    private lateinit var scope: CoroutineScope
    private lateinit var core: TestCore
    private lateinit var sync: AppStore

    @After
    fun tearDown() {
        sync.close()
        scope.cancel()
    }

    private val chat = recording.end.chats.first()
    private val orchestrator = orchestratorKey(chat.id)

    /** 記録の終わりの状態を開いて画面を出す。 */
    private fun open(snapshot: Snapshot = recording.end) {
        core = TestCore(snapshot, recording.events.filter { !it.live && it.seq <= snapshot.seq })
        scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        sync = AppStore(core, scope)
        sync.setForeground(false)
        sync.start()
        rule.setContent {
            YhtyeTheme(darkTheme = true) {
                MainScreen("device", sync, onReconnect = {}, onUnpair = {}, gestureInsets = WindowInsets(0), initialView = MainView.Work)
            }
        }
        scope.launch { sync.openProject(PROJECT.path) }
        settle { sync.state.value.project?.phase == LoadPhase.Ready }
        rule.waitForIdle()
    }

    /**
     * [done] になるまで main looper (同期層のコルーチン) と画面を進める。Compose の `waitUntil` は Robolectric の
     * looper に積まれた (Dispatchers.Main の) コルーチンを流さないことがあるので、自分で回す。
     */
    private fun settle(done: () -> Boolean) {
        repeat(SETTLE_ROUNDS) {
            shadowOf(Looper.getMainLooper()).idle()
            rule.waitForIdle()
            if (done()) return
        }
        error("condition not reached")
    }

    /** オーケストレータがターン中のスナップショット。 */
    private fun working(): Snapshot = recording.end.copy(
        sessions = recording.end.sessions.filter { it.sessionKey != orchestrator } +
            SessionRecord(orchestrator, Role.Orchestrator, null, "acp", SessionStatus.Live, turnRunning = true),
    )

    @Test
    fun `shows the chat and its conversation`() {
        open()
        rule.onNodeWithTag("chat-title").assertTextEquals(chat.title!!)
        rule.onNodeWithTag("chat-branch").assertExists()
        // 最新 (下端) から始まる: オーケストレータの最後の返事が見えている
        rule.onNode(hasText("Done: hello.txt is on main.", substring = true)).assertIsDisplayed()
    }

    @Test
    fun `sending goes through the sync layer and clears the box once accepted`() {
        open()
        rule.onNodeWithTag("composer-input").performTextInput("Please also add a README")
        rule.onNodeWithTag("composer-send").performClick()
        settle { core.callsOf<ApiCommand.SendUserMessage>().isNotEmpty() }
        val sent = core.callsOf<ApiCommand.SendUserMessage>().single()
        assertEquals(chat.id, sent.chat)
        assertEquals("Please also add a README", sent.text)
        rule.waitForIdle()
        assertEquals("", rule.onNodeWithTag("composer-input").fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
    }

    @Test
    fun `a staged attachment shows as a tile and is sent with the message even without text`() {
        open()
        sync.attachments.stage(PROJECT.id, chat.id, "shot.png", "image/png", pngBytes())
        settle { core.attachments.isNotEmpty() && rule.onAllNodes(hasTestTag("attachment-thumb")).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("モデルに直接渡す (native) · 送信時に確定").assertExists()

        rule.onNodeWithTag("composer-send").performClick()
        settle { core.callsOf<ApiCommand.SendUserMessage>().isNotEmpty() }
        val sent = core.callsOf<ApiCommand.SendUserMessage>().single()
        assertEquals("", sent.text)
        assertEquals(listOf("att-1"), sent.attachments)
        settle { rule.onAllNodes(hasTestTag("composer-attachments")).fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun `the x on a tile discards the staged attachment`() {
        open()
        sync.attachments.stage(PROJECT.id, chat.id, "notes.bin", "application/octet-stream", byteArrayOf(1, 2, 3))
        settle { core.attachments.isNotEmpty() }
        rule.waitForIdle()
        rule.onNodeWithContentDescription("notes.bin を外す").performClick()
        settle { core.callsOf<ApiCommand.DiscardAttachment>().isNotEmpty() }
        assertTrue(core.attachments.isEmpty())
        rule.onNodeWithTag("composer-attachments").assertDoesNotExist()
    }

    @Test
    fun `the send button is off while the box is empty`() {
        open()
        rule.onNodeWithTag("composer-send").performClick()
        rule.waitForIdle()
        assertTrue(core.callsOf<ApiCommand.SendUserMessage>().isEmpty())
    }

    @Test
    fun `during a turn the stop button cancels it and sending still works`() {
        open(working())
        rule.onNodeWithText("作業中 · 送信分はターン終了後に届きます").assertExists()
        rule.onNodeWithTag("composer-stop").performClick()
        settle { core.callsOf<ApiCommand.CancelOrchestratorTurn>().isNotEmpty() }
        assertEquals(chat.id, core.callsOf<ApiCommand.CancelOrchestratorTurn>().single().chat)
    }

    @Test
    fun `no stop button when the orchestrator is idle`() {
        open()
        rule.onNodeWithTag("composer-stop").assertDoesNotExist()
    }

    @Test
    fun `a mention from the task panel is sent before the message and a tap on the chip removes it`() {
        open()
        val task = recording.end.state.tasks.first()
        rule.onNodeWithTag("open-right").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("mention-button-${task.id}").performClick()
        rule.waitForIdle()
        // 積んだら中央に戻る
        rule.onNodeWithTag("right-panel").assertDoesNotExist()
        rule.onNodeWithTag("mention-${task.id}").assertIsDisplayed()

        rule.onNodeWithTag("mention-${task.id}").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("mention-${task.id}").assertDoesNotExist()

        rule.onNodeWithTag("open-right").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("mention-button-${task.id}").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("composer-input").performTextInput("look at this")
        rule.onNodeWithTag("composer-send").performClick()
        settle { core.callsOf<ApiCommand.SendUserMessage>().isNotEmpty() }
        assertEquals("@${task.id} ${task.title}\n\nlook at this", core.callsOf<ApiCommand.SendUserMessage>().single().text)
        rule.waitForIdle()
        rule.onNodeWithTag("mention-${task.id}").assertDoesNotExist()
    }

    @Test
    fun `rename asks for the new name and sends it`() {
        open()
        rule.onNodeWithTag("chat-menu").performClick()
        rule.onNodeWithTag("menu-rename").performClick()
        rule.onNodeWithTag("rename-input").performTextReplacement("Greeting file")
        rule.onNodeWithTag("rename-confirm").performClick()
        settle { core.callsOf<ApiCommand.RenameChat>().isNotEmpty() }
        assertEquals("Greeting file", core.callsOf<ApiCommand.RenameChat>().single().title)
        rule.waitForIdle()
        rule.onNodeWithTag("chat-title").assertTextEquals("Greeting file")
        rule.onNodeWithTag("rename-input").assertDoesNotExist()
    }

    @Test
    fun `rename refuses an empty name without asking the core`() {
        open()
        rule.onNodeWithTag("chat-menu").performClick()
        rule.onNodeWithTag("menu-rename").performClick()
        rule.onNodeWithTag("rename-input").performTextReplacement("  ")
        rule.onNodeWithTag("rename-confirm").performClick()
        rule.waitForIdle()
        rule.onNodeWithText("名前を入力してください").assertExists()
        assertTrue(core.callsOf<ApiCommand.RenameChat>().isEmpty())
    }

    @Test
    fun `delete asks first and cancel keeps the chat`() {
        open()
        rule.onNodeWithTag("chat-menu").performClick()
        rule.onNodeWithTag("menu-delete").performClick()
        rule.onNodeWithText("会話の記録は消え、作業ツリーとブランチは残ります。").assertExists()
        rule.onNodeWithText("キャンセル").performClick()
        rule.waitForIdle()
        assertTrue(core.callsOf<ApiCommand.DeleteChat>().isEmpty())

        rule.onNodeWithTag("chat-menu").performClick()
        rule.onNodeWithTag("menu-delete").performClick()
        rule.onNodeWithTag("delete-confirm").performClick()
        settle { core.callsOf<ApiCommand.DeleteChat>().isNotEmpty() }
        assertEquals(chat.id, core.callsOf<ApiCommand.DeleteChat>().single().chat)
        rule.waitForIdle()
        rule.onNodeWithTag("no-chat").assertExists()
    }

    @Test
    fun `delete says why it cannot while the orchestrator works`() {
        open(working())
        rule.onNodeWithTag("chat-menu").performClick()
        rule.onNodeWithTag("menu-delete").performClick()
        rule.onNodeWithText("実行中のため削除できません。先に停止してください。").assertExists()
        rule.onNodeWithTag("delete-confirm").assertDoesNotExist()
    }

    @Test
    fun `sleep is offered for the idle orchestrator and sends the command`() {
        open()
        rule.onNodeWithTag("chat-menu").performClick()
        rule.onNodeWithTag("menu-sleep").performClick()
        settle { core.callsOf<ApiCommand.SleepChat>().isNotEmpty() }
        assertEquals(chat.id, core.callsOf<ApiCommand.SleepChat>().single().chat)
    }

    @Test
    fun `sleep is hidden for a sleeping chat`() {
        open(recording.end.copy(sessions = recording.end.sessions.filter { it.sessionKey != orchestrator }))
        rule.onNodeWithTag("chat-menu").performClick()
        rule.onNodeWithTag("menu-sleep").assertDoesNotExist()
    }

    @Test
    fun `sleep is disabled while the turn runs`() {
        open(working())
        rule.onNodeWithTag("chat-menu").performClick()
        rule.onNodeWithTag("menu-sleep").assertIsNotEnabled()
    }

    @Test
    fun `a project without chats shows how to start`() {
        open(recording.end.copy(chats = emptyList()))
        rule.onNodeWithTag("no-chat").assertExists()
        rule.onNodeWithText(NO_CHAT_HINT).assertExists()
        rule.onNodeWithTag("composer-input").assertDoesNotExist()
    }

    @Test
    fun `the wait banner says how far the group is and opens the task panel`() {
        val state = recording.end.state
        val running = state.tasks.first().let { t ->
            t.copy(status = TaskStatus.Running, current = 0, steps = t.steps.mapIndexed { i, s -> if (i == 0) s.copy(status = StepStatus.Running) else s.copy(status = StepStatus.Pending) })
        }
        val group = state.groups.first().copy(status = GroupStatus.Active)
        open(recording.end.copy(state = state.copy(groups = listOf(group), tasks = listOf(running))))
        rule.onNodeWithText("グループ「${group.title}」の全タスク完了まで待機中 — 0/1 完了").assertExists()
        rule.onNodeWithTag("wait-banner").performClick()
        rule.waitForIdle()
        rule.onNodeWithTag("right-panel").assertExists()
    }

    @Test
    fun `streamed text shows as it arrives`() {
        open(working())
        val cursor = sync.state.value.project!!.cursor
        core.emit(chunk(cursor, "Working on **it**"))
        settle { sync.state.value.project?.streaming?.get(orchestrator) != null }
        rule.waitForIdle()
        rule.onNodeWithTag("streaming").assertExists()
        rule.onNode(hasText("Working on it")).assertExists()
    }

    private fun chunk(seq: Long, text: String): ApiEvent {
        val update = buildJsonObject {
            putJsonObject("content") {
                put("type", "text")
                put("text", text)
            }
        }
        return ApiEvent(seq, 0, PROJECT.id, true, ApiEventBody.Agent(orchestrator, AgentEvent.Output(AgentOutput.MessageChunk(update))))
    }

    private companion object {
        const val SETTLE_ROUNDS = 200
    }
}
