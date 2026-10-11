package io.github.tom039224.yhtye.background

import android.app.Notification
import android.os.Looper
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * Foreground Service: 始めるとすぐ常設の通知を出して前面のサービスになり (5 秒以内に必要)、Session が常駐しないと決めたら
 * 自分で止まる。ここは保存した認証情報が無い (ペアリング前) アプリなので、決まった時点で止まる。
 */
@RunWith(RobolectricTestRunner::class)
class ConnectionServiceTest {
    @Test
    fun `starting it shows the persistent notice at once as a foreground service`() {
        val service = Robolectric.buildService(ConnectionService::class.java).create().get()
        // メインの looper を回す前 (Session がまだペアリングの有無を読んでいない) に始める
        service.onStartCommand(null, 0, 1)
        val shadow = shadowOf(service)
        assertEquals(SystemNotifications.SERVICE_ID, shadow.lastForegroundNotificationId)
        val notification: Notification = assertNotNull(shadow.lastForegroundNotification)
        assertEquals(Channels.CONNECTION, notification.channelId)
        assertEquals("Yhtye: PC に再接続しています…", notification.extras.getString(Notification.EXTRA_TITLE))
        assertTrue(notification.flags and Notification.FLAG_ONGOING_EVENT != 0)
    }

    @Test
    fun `it does not show the notice again once it has stopped even when the answer was already known`() {
        // 先にペアリング前だと分かっている (前のテストで Session が読み終えた状態を作る)
        val app = org.robolectric.RuntimeEnvironment.getApplication() as io.github.tom039224.yhtye.YhtyeApplication
        var tries = 0
        while (app.session.keepAlive.value == null && tries++ < 100) {
            shadowOf(Looper.getMainLooper()).idle()
            Thread.sleep(50)
        }
        val service = Robolectric.buildService(ConnectionService::class.java).create().get()
        service.onStartCommand(null, 0, 1)
        shadowOf(Looper.getMainLooper()).idle()
        val shadow = shadowOf(service)
        assertTrue(shadow.isStoppedBySelf)
        assertTrue(shadow.isForegroundStopped)
        assertEquals(null, shadow.lastForegroundNotification)
    }

    @Test
    fun `it stops itself once the session says nothing is to be kept alive`() {
        val service = Robolectric.buildService(ConnectionService::class.java).create().get()
        service.onStartCommand(null, 0, 1)
        val shadow = shadowOf(service)
        // ペアリングが無い: 保存した認証情報を読み終えたら false になる
        var tries = 0
        while (!shadow.isStoppedBySelf && tries++ < 100) {
            shadowOf(Looper.getMainLooper()).idle()
            Thread.sleep(50)
        }
        assertTrue(shadow.isStoppedBySelf)
        assertTrue(shadow.isForegroundStopped)
    }
}
