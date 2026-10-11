package io.github.tom039224.yhtye.shared.pairing

import io.github.tom039224.yhtye.shared.api.ApiJson
import io.github.tom039224.yhtye.shared.api.generated.PairRequest
import io.github.tom039224.yhtye.shared.api.generated.PairResponse
import io.github.tom039224.yhtye.shared.api.generated.RemoteError
import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.ktor.client.HttpClient
import io.ktor.client.request.post
import io.ktor.client.request.setBody
import io.ktor.client.statement.bodyAsText
import io.ktor.http.ContentType
import io.ktor.http.content.TextContent
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.withTimeoutOrNull

/** ペアリングの結果。 */
sealed interface PairingResult {
    data class Paired(val credentials: Credentials) : PairingResult

    data class Failed(val failure: PairingFailure) : PairingResult
}

/** ペアリングに失敗した理由 (画面で文言を選ぶ)。`message` は PC が返した英語の説明 (補足として出す)。 */
sealed interface PairingFailure {
    /** 401: コードが違う、または有効なコードが無い (期限切れ・使用済み・誤り 5 回)。 */
    data class CodeRejected(val message: String?) : PairingFailure

    /** 403: この端末は PC と同じ Tailscale ユーザーのものではない (か、Tailscale で識別できない)。 */
    data class OtherUser(val message: String?) : PairingFailure

    /** 400: 端末名などが受け付けられない。 */
    data class BadRequest(val message: String?) : PairingFailure

    /** どの接続先にも届かない (Tailscale が切れている、PC のリモート接続がオフ、アドレス違いなど)。 */
    data class Unreachable(val hosts: List<String>, val port: Int, val cause: String?) : PairingFailure

    /**
     * QR の `sid` と違う PC が答えた (接続先の取り違え)。トークンは捨てる。
     * PC 側には端末が登録されてしまっているので、その PC の端末一覧から消してもらう。
     */
    data class WrongServer(val expected: String, val actual: String) : PairingFailure

    /** 想定外の応答 (5xx・形の違う JSON)。 */
    data class ServerError(val status: Int, val message: String?) : PairingFailure
}

/**
 * `POST /pair` でペアリングする。接続先の候補を順に試し、HTTP の応答が返った先で決める
 * (届いた先がコードを拒否したら他の候補は試さない。同じ PC なのでコードの試行回数を無駄に使うだけ)。
 *
 * @param http 平文 HTTP を話せる Ktor のクライアント。`expectSuccess = false` (既定) のまま渡す。
 */
class PairingClient(
    private val http: HttpClient,
    private val attemptTimeoutMs: Long = DEFAULT_ATTEMPT_TIMEOUT_MS,
) {
    suspend fun pair(target: PairingTarget, deviceName: String): PairingResult {
        val name = normalizeDeviceName(deviceName)
            ?: return PairingResult.Failed(PairingFailure.BadRequest("the device name must be 1 to $MAX_DEVICE_NAME_CHARS characters"))
        val body = ApiJson.encodeToString(PairRequest.serializer(), PairRequest(code = target.code, deviceName = name))
        var lastCause: String? = null
        for (host in target.hosts) {
            val reply = try {
                withTimeoutOrNull(attemptTimeoutMs) { post(host, target.port, body) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                lastCause = e.message ?: e::class.simpleName
                continue
            }
            if (reply == null) {
                lastCause = "timed out"
                continue
            }
            return interpret(reply, host, target, name)
        }
        return PairingResult.Failed(PairingFailure.Unreachable(target.hosts, target.port, lastCause))
    }

    private suspend fun post(host: String, port: Int, body: String): HttpReply {
        val response = http.post("http://${TailscaleHosts.urlHost(host)}:$port/pair") {
            setBody(TextContent(body, ContentType.Application.Json))
        }
        return HttpReply(response.status.value, response.bodyAsText())
    }

    private fun interpret(reply: HttpReply, host: String, target: PairingTarget, name: String): PairingResult {
        val failure = when (reply.status) {
            200 -> return paired(reply.body, host, target, name)
            400 -> PairingFailure.BadRequest(errorMessage(reply.body))
            401 -> PairingFailure.CodeRejected(errorMessage(reply.body))
            403 -> PairingFailure.OtherUser(errorMessage(reply.body))
            else -> PairingFailure.ServerError(reply.status, errorMessage(reply.body))
        }
        return PairingResult.Failed(failure)
    }

    private fun paired(body: String, host: String, target: PairingTarget, name: String): PairingResult {
        val response = try {
            ApiJson.decodeFromString(PairResponse.serializer(), body)
        } catch (e: IllegalArgumentException) { // SerializationException を含む
            return PairingResult.Failed(PairingFailure.ServerError(200, "unexpected answer: ${e.message}"))
        }
        val expected = target.serverId
        if (expected != null && expected != response.serverId) {
            return PairingResult.Failed(PairingFailure.WrongServer(expected, response.serverId))
        }
        val credentials = Credentials(
            serverId = response.serverId,
            hosts = listOf(host) + target.hosts.filter { it != host },
            port = target.port,
            deviceId = response.deviceId,
            deviceName = name,
            token = response.token,
        )
        return PairingResult.Paired(credentials)
    }

    private class HttpReply(val status: Int, val body: String)

    companion object {
        /** 1 つの接続先に掛ける時間。届かない候補で長く待たせない。 */
        const val DEFAULT_ATTEMPT_TIMEOUT_MS: Long = 10_000
    }
}

/** 拒否の本文 `{"error": "..."}` の説明。読めなければ null。 */
internal fun errorMessage(body: String): String? =
    try {
        ApiJson.decodeFromString(RemoteError.serializer(), body).error
    } catch (e: IllegalArgumentException) { // SerializationException を含む
        null
    }
