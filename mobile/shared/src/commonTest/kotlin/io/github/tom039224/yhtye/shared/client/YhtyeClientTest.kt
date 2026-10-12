package io.github.tom039224.yhtye.shared.client

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiError
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.TextKind
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.take
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertIs
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class) // runCurrent / advanceTimeBy / currentTime
class YhtyeClientTest {
    private val transport = FakeTransport()
    private val config = YhtyeClient.Config(
        callTimeoutMs = 5_000,
        connectTimeoutMs = 5_000,
        backoff = Backoff(initialMs = 1_000, maxMs = 8_000),
    )

    private fun TestScope.connectedClient(): Pair<YhtyeClient, FakeConnection> {
        val conn = transport.accept()
        val client = YhtyeClient(transport, backgroundScope, config)
        client.start()
        runCurrent()
        assertEquals(ConnectionState.Connected(1), client.state.value)
        return client to conn
    }

    @Test
    fun repliesAreMatchedToRequestsByIdInAnyOrder() = runTest {
        val (client, conn) = connectedClient()
        val ping = async { client.call(ApiCommand.Ping) }
        val list = async { client.call(ApiCommand.ListProjects) }
        runCurrent()
        val first = conn.nextRequest()
        val second = conn.nextRequest()
        assertEquals(ApiCommand.Ping, first.cmd)
        assertEquals(ApiCommand.ListProjects, second.cmd)
        assertTrue(first.id != second.id)
        // 後の要求の応答が先に届く
        conn.reply(second.id, ApiResponse.Projects(emptyList()))
        conn.reply(first.id, ApiResponse.Pong("pc"))
        assertEquals(ApiResponse.Pong("pc"), ping.await())
        assertEquals(ApiResponse.Projects(emptyList()), list.await())
    }

    @Test
    fun anErrorReplyIsThrownAsApiException() = runTest {
        val (client, conn) = connectedClient()
        val call = async { runCatching { client.call(ApiCommand.GetRemoteStatus) } }
        runCurrent()
        val error = ApiError(ApiErrorCode.Forbidden, "this command can only be run on the PC itself")
        conn.fail(conn.nextRequest().id, error)
        val thrown = assertIs<ApiException>(call.await().exceptionOrNull())
        assertEquals(error, thrown.error)
    }

    @Test
    fun aCallWithoutReplyTimesOut() = runTest {
        val (client, conn) = connectedClient()
        val call = async { runCatching { client.call(ApiCommand.Ping, timeoutMs = 2_000) } }
        runCurrent()
        conn.nextRequest()
        advanceTimeBy(2_001)
        assertIs<RpcTimeoutException>(call.await().exceptionOrNull())
        // 遅れて届いた応答は捨てられ、次の要求に影響しない
        val next = async { client.call(ApiCommand.Ping) }
        runCurrent()
        val request = conn.nextRequest()
        conn.reply(request.id - 1, ApiResponse.Pong("late"))
        conn.reply(request.id, ApiResponse.Pong("pc"))
        assertEquals(ApiResponse.Pong("pc"), next.await())
    }

    @Test
    fun eventsAreDelivered() = runTest {
        val (client, conn) = connectedClient()
        val received = async { client.events.take(2).toList() }
        runCurrent()
        val event = ApiEvent(
            seq = 3,
            tsMs = 1,
            project = "p",
            live = true,
            body = ApiEventBody.AgentText(session = "s", kind = TextKind.Message, text = "hi"),
        )
        conn.event(event)
        conn.raw("not json") // 壊れたメッセージは捨てて続ける
        conn.raw("""{"something": "new"}""") // 知らない形も同じ
        conn.event(event.copy(seq = 4))
        assertEquals(listOf(3L, 4L), received.await().map { it.seq })
    }

    @Test
    fun aDroppedConnectionFailsPendingCallsAndReconnectsWithBackoff() = runTest {
        val (client, conn) = connectedClient()
        val call = async { runCatching { client.call(ApiCommand.Ping) } }
        runCurrent()
        conn.nextRequest()
        conn.drop()
        runCurrent()
        assertIs<ConnectionLostException>(call.await().exceptionOrNull())
        assertEquals(ConnectionState.WaitingToReconnect(1, 1_000, "closed by the PC"), client.state.value)
        assertTrue(conn.closedByClient)

        // 1 秒後に試して失敗 → 2 秒、4 秒、8 秒と延び、8 秒 (上限) で止まる
        val start = currentTime
        for (expected in listOf(2_000L, 4_000L, 8_000L)) {
            client.state.first { it is ConnectionState.WaitingToReconnect && it.delayMs == expected }
        }
        assertEquals(1_000L + 2_000 + 4_000, currentTime - start)
        assertEquals(4, transport.attempts)
        advanceTimeBy(8_001)
        assertEquals(5, transport.attempts)
        assertEquals(8_000, assertIs<ConnectionState.WaitingToReconnect>(client.state.value).delayMs)

        // つながれば Connected の番号が変わる (利用側はこれで状態を取り直す)
        val again = transport.accept()
        client.state.first { it is ConnectionState.Connected }
        assertEquals(ConnectionState.Connected(2), client.state.value)
        val ping = async { client.call(ApiCommand.Ping) }
        runCurrent()
        again.reply(again.nextRequest().id, ApiResponse.Pong("pc"))
        assertEquals(ApiResponse.Pong("pc"), ping.await())
    }

    @Test
    fun aCallWaitsForTheConnection() = runTest {
        val client = YhtyeClient(transport, backgroundScope, config)
        client.start()
        runCurrent() // 1 回目は届かない → 1 秒待つ
        val call = async { client.call(ApiCommand.Ping) }
        val conn = transport.accept()
        advanceTimeBy(1_001)
        conn.reply(conn.nextRequest().id, ApiResponse.Pong("pc"))
        assertEquals(ApiResponse.Pong("pc"), call.await())
    }

    @Test
    fun aCallThatCannotConnectInTimeSaysNotConnected() = runTest {
        val client = YhtyeClient(transport, backgroundScope, config)
        client.start()
        val result = runCatching { client.call(ApiCommand.Ping, timeoutMs = 3_000) }
        assertIs<NotConnectedException>(result.exceptionOrNull())
    }

    @Test
    fun unauthorizedMeansRevokedAndStopsReconnecting() = runTest {
        val (client, conn) = connectedClient()
        // PC が失効させて閉じる → 再接続すると 401
        transport.refuse(Refusal.Unauthorized, "missing or unknown device token")
        conn.drop()
        client.state.first { it == ConnectionState.Revoked }
        val attempts = transport.attempts
        advanceTimeBy(120_000)
        client.start() // 失効した後は何もしない
        runCurrent()
        assertEquals(attempts, transport.attempts)
        assertEquals(ConnectionState.Revoked, client.state.value)
        assertIs<NotConnectedException>(runCatching { client.call(ApiCommand.Ping) }.exceptionOrNull())
    }

    @Test
    fun forbiddenStopsUntilStartedAgain() = runTest {
        val client = YhtyeClient(transport, backgroundScope, config)
        transport.refuse(Refusal.Forbidden, "this device belongs to another Tailscale user")
        client.start()
        runCurrent()
        assertEquals(ConnectionState.Rejected("this device belongs to another Tailscale user"), client.state.value)
        advanceTimeBy(120_000)
        assertEquals(1, transport.attempts)
        transport.accept()
        client.start()
        runCurrent()
        assertEquals(ConnectionState.Connected(1), client.state.value)
    }

    @Test
    fun startSkipsTheBackoffWait() = runTest {
        val client = YhtyeClient(transport, backgroundScope, config)
        client.start()
        runCurrent()
        advanceTimeBy(10_000) // 何度か失敗して待ちが延びる
        assertIs<ConnectionState.WaitingToReconnect>(client.state.value)
        transport.accept()
        val before = currentTime
        client.start() // アプリが前面に戻った
        runCurrent()
        assertEquals(ConnectionState.Connected(1), client.state.value)
        assertEquals(before, currentTime)
    }

    @Test
    fun stopDisconnectsAndStartConnectsAgain() = runTest {
        val (client, conn) = connectedClient()
        val call = async { runCatching { client.call(ApiCommand.Ping) } }
        runCurrent()
        client.stop()
        runCurrent()
        assertEquals(ConnectionState.Idle, client.state.value)
        assertTrue(conn.closedByClient)
        assertIs<ConnectionLostException>(call.await().exceptionOrNull())
        transport.accept()
        client.start()
        runCurrent()
        assertEquals(ConnectionState.Connected(2), client.state.value)
    }

    @Test
    fun aHangingConnectAttemptTimesOut() = runTest {
        val hanging = object : WsTransport {
            var attempts = 0
            override suspend fun connect(): WsConnection {
                attempts += 1
                kotlinx.coroutines.awaitCancellation()
            }
        }
        val client = YhtyeClient(hanging, backgroundScope, config)
        client.start()
        advanceTimeBy(5_001)
        assertEquals(ConnectionState.WaitingToReconnect(1, 1_000, "timed out"), client.state.value)
    }
}
