package io.github.tom039224.yhtye.shared.update

import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.MockRequestHandleScope
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.client.request.HttpResponseData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

class ReleaseSourceTest {
    private val requests = mutableListOf<HttpRequestData>()

    private fun source(
        timeoutMs: Long = 5_000,
        handler: suspend MockRequestHandleScope.(HttpRequestData) -> HttpResponseData,
    ): GithubReleaseSource {
        val engine = MockEngine { request ->
            requests += request
            handler(request)
        }
        return GithubReleaseSource(HttpClient(engine), timeoutMs = timeoutMs)
    }

    /** MockEngine は別のディスパッチャで動くので、実時間で動かす。 */
    private fun realTime(block: suspend CoroutineScope.() -> Unit) = runTest { withContext(Dispatchers.Default, block) }

    private fun MockRequestHandleScope.json(status: HttpStatusCode, body: String): HttpResponseData =
        respond(body, status, headersOf(HttpHeaders.ContentType, "application/json"))

    @Test
    fun readsTheTagsAndFlagsAndIgnoresTheRestOfTheResponse() = realTime {
        val body = """
            [
              {"id": 1, "tag_name": "android-v0.1.1", "draft": false, "prerelease": true, "html_url": "https://evil.example/x",
               "assets": [{"name": "yhtye-android-0.1.1.apk"}], "author": {"login": "someone"}},
              {"tag_name": "v1.0.0"}
            ]
        """.trimIndent()
        val releases = source { json(HttpStatusCode.OK, body) }.fetch()
        assertEquals(listOf(GithubRelease("android-v0.1.1", draft = false, prerelease = true), GithubRelease("v1.0.0")), releases)
    }

    @Test
    fun sendsAPlainHttpsGetWithOnlyAnAcceptHeader() = realTime {
        source { json(HttpStatusCode.OK, "[]") }.fetch()
        val request = requests.single()
        assertEquals(HttpMethod.Get, request.method)
        assertEquals("https", request.url.protocol.name)
        assertEquals("api.github.com", request.url.host)
        assertEquals("/repos/Tom039224/yhtye/releases", request.url.encodedPath)
        assertEquals("application/vnd.github+json", request.headers[HttpHeaders.Accept])
        // この端末・PC を表す値や認証は付けない
        assertEquals(null, request.headers[HttpHeaders.Authorization])
        assertEquals(null, request.headers[HttpHeaders.Cookie])
    }

    @Test
    fun refusesAnUrlThatIsNotHttps() {
        val client = HttpClient(MockEngine { json(HttpStatusCode.OK, "[]") })
        assertFailsWith<IllegalArgumentException> { GithubReleaseSource(client, url = "http://api.github.com/repos/Tom039224/yhtye/releases") }
    }

    @Test
    fun failsWithTheStatusWhenGitHubAnswersWithAnError() = realTime {
        val e = assertFailsWith<ReleaseFetchException> { source { json(HttpStatusCode.InternalServerError, "oops") }.fetch() }
        assertEquals("HTTP 500", e.message)
    }

    @Test
    fun saysSoWhenTheRateLimitIsHit() = realTime {
        val limited = assertFailsWith<ReleaseFetchException> { source { json(HttpStatusCode.Forbidden, """{"message":"API rate limit exceeded"}""") }.fetch() }
        assertEquals("HTTP 403 (rate limited?)", limited.message)
        val tooMany = assertFailsWith<ReleaseFetchException> { source { json(HttpStatusCode.TooManyRequests, "") }.fetch() }
        assertEquals("HTTP 429 (rate limited?)", tooMany.message)
    }

    @Test
    fun failsOnAResponseThatIsNotAReleaseList() = realTime {
        assertFailsWith<ReleaseFetchException> { source { json(HttpStatusCode.OK, "<html>captive portal</html>") }.fetch() }
        assertFailsWith<ReleaseFetchException> { source { json(HttpStatusCode.OK, """{"message":"Not Found"}""") }.fetch() }
        assertFailsWith<ReleaseFetchException> { source { json(HttpStatusCode.OK, """[{"draft": false}]""") }.fetch() }
    }

    @Test
    fun failsWhenTheNetworkIsDown() = realTime {
        val e = assertFailsWith<ReleaseFetchException> { source { throw IllegalStateException("Unable to resolve host") }.fetch() }
        assertTrue(e.message.orEmpty().contains("Unable to resolve host"), e.message)
    }

    @Test
    fun givesUpOnAnAnswerThatNeverComes() = realTime {
        val never = CompletableDeferred<Unit>()
        val e = assertFailsWith<ReleaseFetchException> {
            source(timeoutMs = 100) {
                never.await()
                json(HttpStatusCode.OK, "[]")
            }.fetch()
        }
        assertEquals("timed out", e.message)
    }
}
