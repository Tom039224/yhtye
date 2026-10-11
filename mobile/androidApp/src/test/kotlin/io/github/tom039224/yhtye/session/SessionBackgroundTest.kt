package io.github.tom039224.yhtye.session

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import io.github.tom039224.yhtye.background.DeviceSettings
import io.github.tom039224.yhtye.background.SessionNotifications
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.Refusal
import io.github.tom039224.yhtye.shared.client.WsRefusedException
import io.github.tom039224.yhtye.shared.client.WsTransport
import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.github.tom039224.yhtye.shared.credentials.InMemoryCredentialStore
import io.github.tom039224.yhtye.shared.sync.BackgroundMode
import io.github.tom039224.yhtye.shared.sync.MemoryPrefs
import io.github.tom039224.yhtye.shared.sync.UsersTurn
import io.ktor.client.HttpClient
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Session の常駐と通知の配線: 常駐方式と前面/裏で接続するか・切るか、`keepAlive` (Foreground Service を動かすか)、
 * 失効・解除での後始末。PC への接続は偽の transport (つながらない / 拒否する) で。
 */
@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
class SessionBackgroundTest {
    private val app: Application = ApplicationProvider.getApplicationContext()
    private val credentials = Credentials("sid", listOf("100.64.0.9"), 1423, "dev-1", "Pixel", "token")

    private class FakeNotifications : SessionNotifications {
        var revoked = 0
        var cancelAll = 0
        val posted = mutableListOf<UsersTurn>()

        override fun post(turn: UsersTurn) {
            posted += turn
        }

        override fun cancel(project: String, chat: String) = Unit

        override fun revoked() {
            revoked++
        }

        override fun cancelAll() {
            cancelAll++
        }
    }

    /** 試みを数える transport。[refuse] が決まっていれば拒否する。 */
    private class CountingTransport(var refuse: Refusal? = null) : WsTransport {
        var attempts = 0

        override suspend fun connect() = run {
            attempts++
            refuse?.let { throw WsRefusedException(it, null) }
            throw IllegalStateException("unreachable")
        }
    }

    private class Rig(
        scope: TestScope,
        mode: BackgroundMode,
        stored: Credentials?,
        val transport: CountingTransport = CountingTransport(),
    ) {
        val notifications = FakeNotifications()
        val device = DeviceSettings(ApplicationProvider.getApplicationContext<Application>().getSharedPreferences("test-${System.nanoTime()}", 0))
            .also { it.setMode(mode) }
        val store = InMemoryCredentialStore(stored)
        val session = Session(
            store = store,
            http = HttpClient(),
            scope = scope.backgroundScope,
            clientScope = scope.backgroundScope,
            clientPrefs = MemoryPrefs(),
            device = device,
            notifications = notifications,
            transportFor = { transport },
            keepAliveGraceMs = 3_000,
        )

        val client get() = (session.phase.value as Session.Phase.Paired).client
        val connection: ConnectionState get() = client.state.value
    }

    private fun TestScope.rig(mode: BackgroundMode = BackgroundMode.Always, stored: Credentials? = credentials) =
        Rig(this, mode, stored).also { runCurrent() }

    @Test
    fun `without a pairing nothing is kept alive and the hint for the boot receiver is cleared`() = runTest {
        val r = rig(stored = null)
        assertIs<Session.Phase.Unpaired>(r.session.phase.value)
        assertEquals(false, r.session.keepAlive.value)
        assertFalse(r.device.paired)
    }

    @Test
    fun `keepAlive is unknown while the saved pairing is being read`() = runTest {
        val r = Rig(this, BackgroundMode.Always, credentials)
        assertNull(r.session.keepAlive.value)
        runCurrent()
        assertEquals(true, r.session.keepAlive.value)
        assertTrue(r.device.paired)
    }

    @Test
    fun `in the always mode the connection is kept in the background and the service is wanted`() = runTest {
        val r = rig(BackgroundMode.Always)
        // 前面になったことは一度も無い (起動直後・再起動後): それでもつなぐ
        advanceTimeBy(1)
        assertTrue(r.transport.attempts >= 1)
        assertTrue(r.connection !is ConnectionState.Idle)
        r.session.onForeground()
        r.session.onBackground()
        runCurrent()
        assertTrue(r.connection !is ConnectionState.Idle)
        assertEquals(true, r.session.keepAlive.value)
    }

    @Test
    fun `in the work only mode the connection is only for the foreground and nothing is kept alive`() = runTest {
        val r = rig(BackgroundMode.WorkOnly)
        assertEquals(0, r.transport.attempts)
        assertEquals(ConnectionState.Idle, r.connection)
        assertEquals(false, r.session.keepAlive.value)
        r.session.onForeground()
        runCurrent()
        assertTrue(r.transport.attempts >= 1)
        assertTrue(r.connection !is ConnectionState.Idle)
        r.session.onBackground()
        runCurrent()
        assertEquals(ConnectionState.Idle, r.connection)
        assertEquals(false, r.session.keepAlive.value)
    }

    @Test
    fun `switching to work only stops keeping alive after the grace and the connection with it when in the background`() = runTest {
        val r = rig(BackgroundMode.Always)
        assertEquals(true, r.session.keepAlive.value)
        r.device.setMode(BackgroundMode.WorkOnly)
        runCurrent()
        assertEquals(true, r.session.keepAlive.value)
        advanceTimeBy(3_100)
        assertEquals(false, r.session.keepAlive.value)
        assertEquals(ConnectionState.Idle, r.connection)
        // 「常に」に戻すとすぐ保つ
        r.device.setMode(BackgroundMode.Always)
        runCurrent()
        assertEquals(true, r.session.keepAlive.value)
        assertTrue(r.connection !is ConnectionState.Idle)
    }

    @Test
    fun `switching to work only keeps the connection of the foreground`() = runTest {
        val r = rig(BackgroundMode.Always)
        r.session.onForeground()
        r.device.setMode(BackgroundMode.WorkOnly)
        advanceTimeBy(3_100)
        assertEquals(false, r.session.keepAlive.value)
        assertTrue(r.connection !is ConnectionState.Idle)
    }

    @Test
    fun `unpairing stops everything clears the notifications and the boot hint`() = runTest {
        val r = rig(BackgroundMode.Always)
        r.session.unpair()
        runCurrent()
        assertIs<Session.Phase.Unpaired>(r.session.phase.value)
        assertEquals(false, r.session.keepAlive.value)
        assertEquals(1, r.notifications.cancelAll)
        assertFalse(r.device.paired)
        assertNull(r.store.load())
        // 常駐方式は端末の設定なので残る
        assertEquals(BackgroundMode.Always, r.device.mode.value)
    }

    @Test
    fun `a revoked device in the background says so with a notification and stops`() = runTest {
        val r = Rig(this, BackgroundMode.Always, credentials, CountingTransport(Refusal.Unauthorized))
        runCurrent()
        advanceTimeBy(1)
        assertEquals(Session.Phase.Unpaired(revoked = true), r.session.phase.value)
        assertEquals(false, r.session.keepAlive.value)
        assertEquals(1, r.notifications.revoked)
        assertEquals(1, r.notifications.cancelAll)
        assertFalse(r.device.paired)
        assertNull(r.store.load())
    }

    @Test
    fun `a revoked device in the foreground only shows the screen not a notification`() = runTest {
        val r = Rig(this, BackgroundMode.WorkOnly, credentials, CountingTransport(Refusal.Unauthorized))
        r.session.onForeground()
        runCurrent()
        r.session.onForeground()
        runCurrent()
        assertEquals(Session.Phase.Unpaired(revoked = true), r.session.phase.value)
        assertEquals(0, r.notifications.revoked)
    }

    @Test
    fun `a refused device (not the same tailscale user) stops being kept alive`() = runTest {
        val r = Rig(this, BackgroundMode.Always, credentials, CountingTransport(Refusal.Forbidden))
        runCurrent()
        advanceTimeBy(3_100)
        assertIs<ConnectionState.Rejected>(r.connection)
        assertEquals(false, r.session.keepAlive.value)
    }

    @Test
    fun `an open chat request waits until the screen takes it and a later one replaces it`() = runTest {
        val r = rig()
        assertNull(r.session.openChat.value)
        r.session.requestOpenChat("repo", "C-1")
        val first = r.session.openChat.value!!
        assertEquals("repo" to "C-1", first.project to first.chat)
        r.session.requestOpenChat("repo", "C-1")
        val second = r.session.openChat.value!!
        assertTrue(second.serial > first.serial)
        // 古い頼みを済ませても、新しい頼みは消えない
        r.session.consumeOpenChat(first)
        assertEquals(second, r.session.openChat.value)
        r.session.consumeOpenChat(second)
        assertNull(r.session.openChat.value)
    }

    @Test
    fun `the persistent notice follows the connection`() = runTest {
        val r = rig()
        assertEquals(false, r.session.connected.value)
    }
}
