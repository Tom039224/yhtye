package io.github.tom039224.yhtye.shared.client

import io.ktor.client.HttpClient
import io.ktor.client.engine.okhttp.OkHttp
import io.ktor.client.plugins.websocket.WebSockets
import java.util.concurrent.TimeUnit

/** OkHttp の接続のタイムアウト。Tailscale の相手が落ちているとき長く待たない。 */
private const val CONNECT_TIMEOUT_SECONDS = 10L

/** WebSocket の ping の間隔。モバイル回線の切り替えなどで黙って死んだ接続を見つける。 */
private const val PING_INTERVAL_SECONDS = 15L

/**
 * PC と話す Ktor のクライアント (OkHttp)。ペアリング ([io.github.tom039224.yhtye.shared.pairing.PairingClient]) と
 * WebSocket ([KtorWsTransport]) で共有する。
 */
fun createHttpClient(): HttpClient = HttpClient(OkHttp) {
    expectSuccess = false
    install(WebSockets)
    engine {
        config {
            connectTimeout(CONNECT_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            // OkHttp では ping は WebSockets プラグインではなくエンジンで設定する
            pingInterval(PING_INTERVAL_SECONDS, TimeUnit.SECONDS)
            retryOnConnectionFailure(false)
        }
    }
}
