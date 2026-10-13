package io.github.tom039224.yhtye.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import io.github.tom039224.yhtye.background.OpenChatRequest
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertFalse

/** 完了の通知のタップ: そのプロジェクトのそのチャットを中央に出し、開いているパネルと積んだ画面を閉じる。 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w400dp-h800dp")
class NotificationTapTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val h = MainHarness(rule)

    @After
    fun tearDown() = h.close()

    private val other = ProjectInfo(id = "beta", name = "beta-repo", path = "/tmp/beta", open = true, running = false, awake = false)

    private fun openTwoChats() {
        val first = h.recording.end.chats.first()
        val second = ChatInfo("C-2", first.worktree, "Second chat", createdMs = 1, lastUsedMs = 1)
        h.open(h.recording.end.copy(chats = listOf(first, second))) { projects = listOf(PROJECT, other) }
    }

    @Test
    fun `a tap shows the chat of the notification and reports the request handled`() {
        openTwoChats()
        val request = OpenChatRequest(PROJECT.id, "C-2", 1)
        h.openChat.value = request
        h.settle { h.handled.isNotEmpty() }
        assertEquals(listOf(request), h.handled)
        assertEquals("C-2", h.sync.state.value.project?.selectedChat)
        rule.onNodeWithTag("chat-title").assertTextEquals("Second chat")
    }

    @Test
    fun `a tap closes the open panel and the settings screen first`() {
        openTwoChats()
        h.openLeft()
        rule.onNodeWithTag("rail-settings").performClick()
        h.settle { h.exists("settings") }
        h.openChat.value = OpenChatRequest(PROJECT.id, "C-2", 1)
        h.settle { h.handled.isNotEmpty() }
        h.settle { !h.exists("settings") }
        assertFalse(h.exists("left-panel"))
        assertEquals("C-2", h.sync.state.value.project?.selectedChat)
    }

    @Test
    fun `a tap for a chat of another project opens that project`() {
        openTwoChats()
        h.openChat.value = OpenChatRequest(other.id, "C-1", 1)
        h.settle { h.handled.isNotEmpty() }
        assertEquals(other.id, h.sync.state.value.project?.info?.id)
        assertEquals(listOf(other.path), h.core.callsOf<ApiCommand.OpenProject>().map { it.path }.filter { it == other.path })
    }

    @Test
    fun `a tap for a chat that no longer exists leaves the screen where it is but is still handled`() {
        openTwoChats()
        h.openChat.value = OpenChatRequest(PROJECT.id, "C-404", 1)
        h.settle { h.handled.isNotEmpty() }
        assertEquals(PROJECT.id, h.sync.state.value.project?.info?.id)
        assertEquals(h.recording.end.chats.first().id, h.sync.state.value.project?.selectedChat)
    }
}
