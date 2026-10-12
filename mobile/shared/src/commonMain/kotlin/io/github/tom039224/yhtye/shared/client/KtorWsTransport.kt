package io.github.tom039224.yhtye.shared.client

import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.github.tom039224.yhtye.shared.pairing.TailscaleHosts
import io.github.tom039224.yhtye.shared.pairing.errorMessage
import io.ktor.client.HttpClient
import io.ktor.client.plugins.websocket.DefaultClientWebSocketSession
import io.ktor.client.plugins.websocket.webSocketSession
import io.ktor.client.request.get
import io.ktor.client.request.header
import io.ktor.client.statement.bodyAsText
import io.ktor.http.HttpHeaders
import io.ktor.websocket.Frame
import io.ktor.websocket.close
import io.ktor.websocket.readText
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.channels.ClosedReceiveChannelException
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.concurrent.Volatile

/**
 * Ktor で PC の `GET /ws` につなぐ。[Credentials.hosts] を順に試す (前回つながった先を先に)。
 *
 * WebSocket のハンドシェイクが失敗したとき、エンジンによっては HTTP の状態コードが取れない
 * (OkHttp では 403 が例外の文言にしか残らない)。そこで同じ先へ普通の `GET /ws` を送って拒否の理由を確かめる:
 * PC は upgrade の前に whois (403) と端末トークン (401) を検査するので、同じ答えが返る。
 *
 * @param http WebSockets プラグインを入れた Ktor のクライアント (`expectSuccess = false`)。
 */
class KtorWsTransport(
    private val http: HttpClient,
    private val credentials: Credentials,
    private val probeTimeoutMs: Long = DEFAULT_PROBE_TIMEOUT_MS,
) : WsTransport {
    /** 前回つながった先。[YhtyeClient] は 1 度に 1 つしか [connect] しない。 */
    @Volatile
    private var preferred: String? = null

    override suspend fun connect(): WsConnection {
        var lastError: Exception? = null
        val hosts = credentials.hosts.sortedByDescending { it == preferred }
        for (host in hosts) {
            try {
                val session = http.webSocketSession(url(host, "ws")) {
                    header(HttpHeaders.Authorization, "Bearer ${credentials.token}")
                }
                preferred = host
                return KtorWsConnection(session)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                probe(host)?.let { throw it }
                lastError = e
            }
        }
        throw lastError ?: IllegalStateException("no host to connect to")
    }

    /** ハンドシェイクが失敗した先が接続を拒否したのなら、その理由。届かない・分からないなら null。 */
    private suspend fun probe(host: String): WsRefusedException? {
        val reply = try {
            withTimeoutOrNull(probeTimeoutMs) {
                val response = http.get(url(host, "http")) {
                    header(HttpHeaders.Authorization, "Bearer ${credentials.token}")
                }
                response.status.value to response.bodyAsText()
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            null
        } ?: return null
        val (status, body) = reply
        return when (status) {
            401 -> WsRefusedException(Refusal.Unauthorized, errorMessage(body))
            403 -> WsRefusedException(Refusal.Forbidden, errorMessage(body))
            else -> null
        }
    }

    private fun url(host: String, scheme: String): String =
        "$scheme://${TailscaleHosts.urlHost(host)}:${credentials.port}$WS_PATH"

    companion object {
        const val WS_PATH: String = "/ws"
        const val DEFAULT_PROBE_TIMEOUT_MS: Long = 10_000
    }
}

private class KtorWsConnection(private val session: DefaultClientWebSocketSession) : WsConnection {
    override suspend fun receive(): String? {
        while (true) {
            val frame = try {
                session.incoming.receive()
            } catch (e: ClosedReceiveChannelException) {
                return null
            }
            when (frame) {
                is Frame.Text -> return frame.readText()
                is Frame.Close -> return null
                else -> Unit // ping / pong はエンジンが扱う。バイナリは使わない
            }
        }
    }

    override suspend fun send(text: String) {
        session.send(Frame.Text(text))
    }

    override suspend fun close() {
        session.close()
    }
}
