package io.github.tom039224.yhtye.shared.update

import io.ktor.client.HttpClient
import io.ktor.client.request.get
import io.ktor.client.request.header
import io.ktor.client.statement.bodyAsText
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.isSuccess
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.SerializationException
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json

/** リリースの一覧を読めなかった (オフライン・API の回数制限・GitHub の不調・読めない応答)。`message` は設定画面に出す短い理由。 */
class ReleaseFetchException(reason: String, cause: Throwable? = null) : Exception(reason, cause)

/** リリースの一覧の取り出し口 (テストは偽物に替える)。 */
fun interface ReleaseSource {
    /** @throws ReleaseFetchException 読めなかったとき。 */
    suspend fun fetch(): List<GithubRelease>
}

/**
 * GitHub の API から読む。[url] は HTTPS だけ (平文を許す Tailscale 向けの通信とは別の、公開の相手)。
 * 送るのは普通の GET と `Accept` だけで、この端末・PC を表す値は付けない。
 */
class GithubReleaseSource(
    private val http: HttpClient,
    private val url: String = RELEASES_URL,
    private val timeoutMs: Long = TIMEOUT_MS,
) : ReleaseSource {
    init {
        require(url.startsWith("https://")) { "the update check talks HTTPS only: $url" }
    }

    override suspend fun fetch(): List<GithubRelease> {
        val body = try {
            withTimeout(timeoutMs) {
                val response = http.get(url) { header(HttpHeaders.Accept, ACCEPT) }
                if (!response.status.isSuccess()) throw ReleaseFetchException(describe(response.status))
                response.bodyAsText()
            }
        } catch (e: ReleaseFetchException) {
            throw e
        } catch (e: TimeoutCancellationException) {
            throw ReleaseFetchException("timed out", e)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            throw ReleaseFetchException(e.message ?: e::class.simpleName ?: "failed", e)
        }
        return try {
            // 100 件のリリースの JSON は大きいので、呼び出し元 (メインスレッド) で解析しない
            withContext(Dispatchers.Default) { JSON.decodeFromString(ListSerializer(GithubRelease.serializer()), body) }
        } catch (e: SerializationException) {
            throw ReleaseFetchException("unexpected response", e)
        }
    }

    private fun describe(status: HttpStatusCode): String = when (status.value) {
        // 認証なしの API は 1 時間に 60 回まで。超えると 403 (または 429) が返る
        HttpStatusCode.Forbidden.value, HttpStatusCode.TooManyRequests.value -> "HTTP ${status.value} (rate limited?)"
        else -> "HTTP ${status.value}"
    }

    private companion object {
        const val TIMEOUT_MS = 15_000L
        const val ACCEPT = "application/vnd.github+json"
        val JSON = Json { ignoreUnknownKeys = true }
    }
}
