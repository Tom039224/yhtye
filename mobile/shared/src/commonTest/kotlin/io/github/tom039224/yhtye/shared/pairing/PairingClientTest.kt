package io.github.tom039224.yhtye.shared.pairing

import io.github.tom039224.yhtye.shared.api.ApiJson
import io.github.tom039224.yhtye.shared.api.generated.PairRequest
import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.MockRequestHandleScope
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.client.request.HttpResponseData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.content.TextContent
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.withContext
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

class PairingClientTest {
    private val target = PairingTarget(
        hosts = listOf("pc.tail1234.ts.net", "100.89.117.15"),
        port = 1423,
        code = "ABCD2345",
        serverId = "sid1",
    )
    private val requests = mutableListOf<HttpRequestData>()

    private fun client(handler: suspend MockRequestHandleScope.(HttpRequestData) -> HttpResponseData): PairingClient {
        val engine = MockEngine { request ->
            requests += request
            handler(request)
        }
        return PairingClient(HttpClient(engine), attemptTimeoutMs = 1_000)
    }

    /**
     * MockEngine は別のディスパッチャで動くので、runTest の仮想時間のままだとタイムアウトが先に来てしまう。
     * 実時間で動かす。
     */
    private fun realTime(block: suspend CoroutineScope.() -> Unit) = runTest {
        withContext(Dispatchers.Default, block)
    }

    private fun MockRequestHandleScope.json(status: HttpStatusCode, body: String): HttpResponseData =
        respond(body, status, headersOf(HttpHeaders.ContentType, "application/json"))

    private val paired = """{"device_id": "dev-1", "token": "t0k3n", "server_id": "sid1"}"""

    @Test
    fun pairingSendsTheCodeAndTheNameAndKeepsTheToken() = realTime {
        val result = client { json(HttpStatusCode.OK, paired) }.pair(target, "  Pixel 9 ")
        val credentials = assertIs<PairingResult.Paired>(result).credentials
        assertEquals("dev-1", credentials.deviceId)
        assertEquals("t0k3n", credentials.token)
        assertEquals("sid1", credentials.serverId)
        assertEquals("Pixel 9", credentials.deviceName)
        assertEquals(target.hosts, credentials.hosts)
        assertEquals(1423, credentials.port)
        val request = requests.single()
        assertEquals(HttpMethod.Post, request.method)
        assertEquals("http://pc.tail1234.ts.net:1423/pair", request.url.toString())
        val body = ApiJson.decodeFromString(PairRequest.serializer(), (request.body as TextContent).text)
        assertEquals(PairRequest(code = "ABCD2345", deviceName = "Pixel 9"), body)
    }

    @Test
    fun anUnreachableHostIsSkippedAndTheOneThatAnsweredComesFirst() = realTime {
        val result = client { request ->
            if (request.url.host == "pc.tail1234.ts.net") error("Unable to resolve host")
            json(HttpStatusCode.OK, paired)
        }.pair(target, "Pixel")
        val credentials = assertIs<PairingResult.Paired>(result).credentials
        assertEquals(listOf("100.89.117.15", "pc.tail1234.ts.net"), credentials.hosts)
        assertEquals(2, requests.size)
    }

    @Test
    fun aHostThatHangsTimesOutAndTheNextIsTried() = realTime {
        val result = client { request ->
            if (request.url.host == "pc.tail1234.ts.net") awaitCancellation()
            json(HttpStatusCode.OK, paired)
        }.pair(target, "Pixel")
        assertIs<PairingResult.Paired>(result)
    }

    @Test
    fun aRefusedCodeIsNotRetriedOnTheOtherHost() = realTime {
        val result = client { json(HttpStatusCode.Unauthorized, """{"error": "wrong pairing code"}""") }.pair(target, "Pixel")
        assertEquals(PairingFailure.CodeRejected("wrong pairing code"), assertIs<PairingResult.Failed>(result).failure)
        assertEquals(1, requests.size)
    }

    @Test
    fun eachRefusalHasItsKind() = realTime {
        val forbidden = client { json(HttpStatusCode.Forbidden, """{"error": "other user"}""") }.pair(target, "P")
        assertEquals(PairingFailure.OtherUser("other user"), assertIs<PairingResult.Failed>(forbidden).failure)
        val bad = client { json(HttpStatusCode.BadRequest, """{"error": "bad name"}""") }.pair(target, "P")
        assertEquals(PairingFailure.BadRequest("bad name"), assertIs<PairingResult.Failed>(bad).failure)
        val broken = client { json(HttpStatusCode.InternalServerError, "oops") }.pair(target, "P")
        assertEquals(PairingFailure.ServerError(500, null), assertIs<PairingResult.Failed>(broken).failure)
        val garbage = client { json(HttpStatusCode.OK, "{}") }.pair(target, "P")
        assertIs<PairingFailure.ServerError>(assertIs<PairingResult.Failed>(garbage).failure)
    }

    @Test
    fun anotherPcThanTheQrCodesIsRefused() = realTime {
        val result = client { json(HttpStatusCode.OK, paired.replace("sid1", "sid2")) }.pair(target, "Pixel")
        assertEquals(PairingFailure.WrongServer("sid1", "sid2"), assertIs<PairingResult.Failed>(result).failure)
        // 手入力 (sid 無し) ならどの PC でも受け入れる
        val manual = client { json(HttpStatusCode.OK, paired.replace("sid1", "sid2")) }.pair(target.copy(serverId = null), "Pixel")
        assertEquals("sid2", assertIs<PairingResult.Paired>(manual).credentials.serverId)
    }

    @Test
    fun noHostReachableIsReported() = realTime {
        val result = client { error("connect failed") }.pair(target, "Pixel")
        val failure = assertIs<PairingFailure.Unreachable>(assertIs<PairingResult.Failed>(result).failure)
        assertEquals(target.hosts, failure.hosts)
        assertEquals(2, requests.size)
    }

    @Test
    fun aBadDeviceNameIsRefusedBeforeSending() = realTime {
        val result = client { json(HttpStatusCode.OK, paired) }.pair(target, "   ")
        assertIs<PairingFailure.BadRequest>(assertIs<PairingResult.Failed>(result).failure)
        assertEquals(0, requests.size)
    }
}
