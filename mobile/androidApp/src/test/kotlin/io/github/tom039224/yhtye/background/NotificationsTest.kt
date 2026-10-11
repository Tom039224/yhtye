package io.github.tom039224.yhtye.background

import android.Manifest
import android.app.Application
import android.app.Notification
import android.app.NotificationManager
import android.content.Context
import androidx.test.core.app.ApplicationProvider
import io.github.tom039224.yhtye.shared.sync.BackgroundMode
import io.github.tom039224.yhtye.shared.sync.UsersTurn
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 完了の通知: 文・チャネル・チャットごとに 1 つ・タップで開く先・許可が無ければ出さない。 */
@RunWith(RobolectricTestRunner::class)
class NotificationsTest {
    private val app: Application = ApplicationProvider.getApplicationContext()
    private val manager = app.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    private val notifications = SystemNotifications(app)

    private val turn = UsersTurn("repo", "C-1", "Refactor checkout", "Done: hello.txt is on main.\nMore details", 1_800_000_000_000)

    @Before
    fun setUp() {
        shadowOf(app).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
        notifications.ensureChannels()
    }

    private fun posted(project: String = "repo", chat: String = "C-1"): Notification? =
        shadowOf(manager).getNotification(SystemNotifications.turnTag(project, chat), 1)

    @Test
    fun `creates the three channels with a quiet one for the persistent notice and a loud one for completions`() {
        assertEquals(NotificationManager.IMPORTANCE_LOW, manager.getNotificationChannel(Channels.CONNECTION).importance)
        assertEquals(NotificationManager.IMPORTANCE_HIGH, manager.getNotificationChannel(Channels.WORK_DONE).importance)
        assertEquals(NotificationManager.IMPORTANCE_DEFAULT, manager.getNotificationChannel(Channels.ALERT).importance)
    }

    @Test
    fun `the notification names the chat and shows the first line of the last words`() {
        notifications.post(turn)
        val n = assertNotNull(posted())
        assertEquals("Refactor checkout の作業が完了しました", n.extras.getString(Notification.EXTRA_TITLE))
        assertEquals("Done: hello.txt is on main.", n.extras.getCharSequence(Notification.EXTRA_TEXT)?.toString())
        assertEquals(Channels.WORK_DONE, n.channelId)
        assertTrue(n.flags and Notification.FLAG_AUTO_CANCEL != 0)
    }

    @Test
    fun `an untitled chat gets the new chat title and no body without last words`() {
        notifications.post(turn.copy(title = null, lastMessage = null))
        val n = assertNotNull(posted())
        assertEquals("新しいチャット の作業が完了しました", n.extras.getString(Notification.EXTRA_TITLE))
        assertNull(n.extras.getCharSequence(Notification.EXTRA_TEXT))
    }

    @Test
    fun `the same chat is replaced and another chat or project gets its own`() {
        notifications.post(turn)
        notifications.post(turn.copy(lastMessage = "Second time"))
        assertEquals(1, shadowOf(manager).allNotifications.size)
        assertEquals("Second time", posted()?.extras?.getCharSequence(Notification.EXTRA_TEXT)?.toString())
        notifications.post(turn.copy(chat = "C-2"))
        notifications.post(turn.copy(project = "beta"))
        assertEquals(3, shadowOf(manager).allNotifications.size)
    }

    @Test
    fun `the tap opens that chat of that project`() {
        notifications.post(turn)
        notifications.post(turn.copy(project = "beta", chat = "C-7"))
        val tap = { project: String, chat: String -> shadowOf(assertNotNull(posted(project, chat)).contentIntent).savedIntent }
        assertEquals("repo" to "C-1", OpenChatIntent.parse(tap("repo", "C-1")))
        assertEquals("beta" to "C-7", OpenChatIntent.parse(tap("beta", "C-7")))
        // 別々の PendingIntent (extras だけが違うと同じものに潰れる)
        assertTrue(shadowOf(posted("repo", "C-1")!!.contentIntent).requestCode != shadowOf(posted("beta", "C-7")!!.contentIntent).requestCode)
    }

    @Test
    fun `an intent that is not an open chat request is not read`() {
        assertNull(OpenChatIntent.parse(null))
        assertNull(OpenChatIntent.parse(android.content.Intent(android.content.Intent.ACTION_VIEW)))
        assertNull(OpenChatIntent.parse(android.content.Intent(OpenChatIntent.ACTION)))
    }

    @Test
    fun `cancelling one chat leaves the others and cancelling all removes the completions but not the notices`() {
        notifications.post(turn)
        notifications.post(turn.copy(chat = "C-2"))
        notifications.revoked()
        notifications.cancel("repo", "C-1")
        assertNull(posted("repo", "C-1"))
        assertNotNull(posted("repo", "C-2"))
        notifications.cancelAll()
        assertNull(posted("repo", "C-2"))
        // 失効の通知は残る (ペアリングの案内)
        assertEquals(1, shadowOf(manager).allNotifications.size)
    }

    @Test
    fun `nothing is posted without the permission or with notifications turned off`() {
        shadowOf(app).denyPermissions(Manifest.permission.POST_NOTIFICATIONS)
        notifications.post(turn)
        assertEquals(0, shadowOf(manager).allNotifications.size)
        shadowOf(app).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
        shadowOf(manager).setNotificationsEnabled(false)
        notifications.post(turn)
        assertEquals(0, shadowOf(manager).allNotifications.size)
    }

    @Test
    fun `the persistent notice is ongoing silent and says the mode and whether the pc is connected`() {
        val connected = notifications.serviceNotification(connected = true, mode = BackgroundMode.Always)
        assertEquals("Yhtye: PC に接続中", connected.extras.getString(Notification.EXTRA_TITLE))
        assertEquals("常に接続", connected.extras.getCharSequence(Notification.EXTRA_TEXT)?.toString())
        assertEquals(Channels.CONNECTION, connected.channelId)
        assertTrue(connected.flags and Notification.FLAG_ONGOING_EVENT != 0)
        assertNotNull(connected.contentIntent)
        val waiting = notifications.serviceNotification(connected = false, mode = BackgroundMode.WorkOnly)
        assertEquals("Yhtye: PC に再接続しています…", waiting.extras.getString(Notification.EXTRA_TITLE))
        assertEquals("作業が終わるまで接続", waiting.extras.getCharSequence(Notification.EXTRA_TEXT)?.toString())
    }

    @Test
    fun `the revoked notice points the user at pairing again`() {
        notifications.revoked()
        val n = assertNotNull(shadowOf(manager).getNotification("revoked", 2))
        assertEquals("Yhtye: この端末は PC から外されました", n.extras.getString(Notification.EXTRA_TITLE))
        assertEquals(Channels.ALERT, n.channelId)
        assertNotNull(n.contentIntent)
    }
}
