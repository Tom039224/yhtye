package io.github.tom039224.yhtye.session

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import io.github.tom039224.yhtye.background.DeviceSettings
import io.github.tom039224.yhtye.background.SessionNotifications
import io.github.tom039224.yhtye.shared.client.Refusal
import io.github.tom039224.yhtye.shared.client.WsRefusedException
import io.github.tom039224.yhtye.shared.client.WsTransport
import io.github.tom039224.yhtye.shared.credentials.CredentialStore
import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.github.tom039224.yhtye.shared.pairing.PairingResult
import io.github.tom039224.yhtye.shared.pairing.PairingTarget
import io.github.tom039224.yhtye.shared.sync.MemoryPrefs
import io.github.tom039224.yhtye.shared.sync.UsersTurn
import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import kotlin.test.assertEquals
import kotlin.test.assertIs

/**
 * ペアリングの解除・失効のあと、すぐに新しくペアリングしたとき。古い認証情報の削除は画面を戻したあとで走るので、
 * 遅れて終わっても、新しい認証情報を消さない。
 */
@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(RobolectricTestRunner::class)
class SessionPairingTest {
    private val old = Credentials("sid", listOf("100.64.0.9"), 1423, "dev-1", "Pixel", "old-token")
    private val target = PairingTarget(listOf("100.64.0.9"), 1423, "ABCD2345", "sid")

    /**
     * 削除が、新しいペアリングの応答 ([pairReplied]) のあとまで終わらない保存先 (Keystore の鍵の削除が遅いことを想定)。
     * 応答のあと、保存まで進む余裕 ([AFTER_REPLY_MS]) を本物の時間で待つ (PC への通信も本物の時間で動くので)。
     */
    private class SlowClearStore(initial: Credentials?, private val pairReplied: Deferred<Unit>) : CredentialStore {
        var stored: Credentials? = initial
            private set

        /** 削除が終わった。 */
        val cleared = CompletableDeferred<Unit>()

        override suspend fun load(): Credentials? = stored

        override suspend fun save(credentials: Credentials) {
            stored = credentials
        }

        override suspend fun clear() {
            withContext(Dispatchers.Default) {
                pairReplied.await()
                delay(AFTER_REPLY_MS)
            }
            stored = null
            cleared.complete(Unit)
        }
    }

    private class QuietNotifications : SessionNotifications {
        override fun post(turn: UsersTurn) = Unit

        override fun cancel(project: String, chat: String) = Unit

        override fun revoked() = Unit

        override fun cancelAll() = Unit
    }

    /** 最初の接続だけ拒否する (失効)。つなぎ直したもの (新しいペアリング) は拒否しない。 */
    private class RevokeOnce : (Credentials) -> WsTransport {
        private var used = false

        override fun invoke(credentials: Credentials): WsTransport {
            val revoke = !used
            used = true
            return WsTransport {
                if (revoke) throw WsRefusedException(Refusal.Unauthorized, null)
                throw IllegalStateException("unreachable")
            }
        }
    }

    private val pairedReply = """{"device_id": "dev-2", "token": "new-token", "server_id": "sid"}"""

    private val pairReplied = CompletableDeferred<Unit>()

    private fun TestScope.session(store: CredentialStore, transportFor: (Credentials) -> WsTransport): Session {
        val http = HttpClient(
            MockEngine {
                pairReplied.complete(Unit)
                respond(pairedReply, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
            },
        )
        val prefs = ApplicationProvider.getApplicationContext<Application>().getSharedPreferences("pairing-${System.nanoTime()}", 0)
        return Session(
            store = store,
            http = http,
            scope = backgroundScope,
            clientScope = backgroundScope,
            clientPrefs = MemoryPrefs(),
            device = DeviceSettings(prefs),
            notifications = QuietNotifications(),
            transportFor = transportFor,
        ).also { runCurrent() }
    }

    private suspend fun Session.pairAgain(): Credentials {
        val result = withContext(Dispatchers.Default) { pair(target, "Pixel 9") }
        return assertIs<PairingResult.Paired>(result).credentials
    }

    @Test
    fun `pairing right after unpairing keeps the new credentials though the old ones are still being deleted`() = runTest {
        val store = SlowClearStore(old, pairReplied)
        val session = session(store) { WsTransport { throw IllegalStateException("unreachable") } }
        assertIs<Session.Phase.Paired>(session.phase.value)

        session.unpair()
        assertIs<Session.Phase.Unpaired>(session.phase.value)
        val fresh = session.pairAgain()
        store.cleared.await() // 古い削除が終わったあと
        runCurrent()

        assertEquals("new-token", fresh.token)
        assertEquals(fresh, store.stored)
        assertEquals(fresh, (session.phase.value as Session.Phase.Paired).credentials)
    }

    @Test
    fun `pairing right after a revocation keeps the new credentials though the old ones are still being deleted`() = runTest {
        val store = SlowClearStore(old, pairReplied)
        val session = session(store, RevokeOnce())
        advanceTimeBy(1)
        assertEquals(Session.Phase.Unpaired(revoked = true), session.phase.value)

        val fresh = session.pairAgain()
        store.cleared.await() // 古い削除が終わったあと
        runCurrent()

        assertEquals(fresh, store.stored)
        assertEquals(fresh, (session.phase.value as Session.Phase.Paired).credentials)
    }

    private companion object {
        /** ペアリングの応答のあと、削除を終えるまでの本物の時間。応答から保存までは数ミリ秒なので十分長く。 */
        const val AFTER_REPLY_MS = 200L
    }
}
