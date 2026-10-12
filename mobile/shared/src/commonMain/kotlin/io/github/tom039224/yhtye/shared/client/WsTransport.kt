package io.github.tom039224.yhtye.shared.client

/**
 * PC の `/ws` への接続を作るもの。[YhtyeClient] はこれ越しに話す (テストでは偽物に差し替える)。
 */
fun interface WsTransport {
    /**
     * 接続する。PC が拒否したら [WsRefusedException]、届かない等はその他の例外。
     */
    suspend fun connect(): WsConnection
}

/** 1 本の WebSocket (テキストのメッセージだけを扱う)。 */
interface WsConnection {
    /** 次のテキストメッセージ。閉じたら null (または例外)。 */
    suspend fun receive(): String?

    suspend fun send(text: String)

    /** 閉じる (何度呼んでもよい)。 */
    suspend fun close()
}

/** PC が接続を拒否した理由。 */
enum class Refusal {
    /** 401: 端末トークンが無効 (この端末は PC から外された)。 */
    Unauthorized,

    /** 403: この端末は PC と同じ Tailscale ユーザーのものではない (か、識別できない)。 */
    Forbidden,
}

class WsRefusedException(val refusal: Refusal, message: String?) :
    Exception(message ?: refusal.name)
