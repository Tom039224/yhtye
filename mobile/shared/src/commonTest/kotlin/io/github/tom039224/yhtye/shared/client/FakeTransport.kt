package io.github.tom039224.yhtye.shared.client

import io.github.tom039224.yhtye.shared.api.ApiJson
import io.github.tom039224.yhtye.shared.api.generated.ApiError
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.WsReply
import io.github.tom039224.yhtye.shared.api.generated.WsRequest
import io.github.tom039224.yhtye.shared.api.generated.WsServerMessage
import kotlinx.coroutines.channels.Channel

/** 接続の試みごとの結果を順に返す偽の transport。台本が尽きたら「届かない」。 */
class FakeTransport : WsTransport {
    private val script = ArrayDeque<() -> WsConnection>()
    var attempts = 0
        private set

    /** 次の試みでつながる。 */
    fun accept(): FakeConnection = FakeConnection().also { conn -> script.addLast { conn } }

    /** 次の試みで PC が拒否する。 */
    fun refuse(refusal: Refusal, message: String? = null) {
        script.addLast { throw WsRefusedException(refusal, message) }
    }

    override suspend fun connect(): WsConnection {
        attempts += 1
        val next = script.removeFirstOrNull() ?: throw IllegalStateException("unreachable")
        return next()
    }
}

/** PC 側から操作できる偽の接続。 */
class FakeConnection : WsConnection {
    private val toClient = Channel<String>(Channel.UNLIMITED)
    private val fromClient = Channel<String>(Channel.UNLIMITED)
    var closedByClient = false
        private set

    override suspend fun receive(): String? = toClient.receiveCatching().getOrNull()

    override suspend fun send(text: String) {
        check(!closedByClient) { "closed" }
        fromClient.send(text)
    }

    override suspend fun close() {
        closedByClient = true
        toClient.close()
    }

    /** クライアントが送った次の要求。 */
    suspend fun nextRequest(): WsRequest = ApiJson.decodeFromString(WsRequest.serializer(), fromClient.receive())

    fun reply(id: Long, response: ApiResponse) =
        push(WsServerMessage.Reply(WsReply.Ok(id, response)))

    fun fail(id: Long, error: ApiError) =
        push(WsServerMessage.Reply(WsReply.Err(id, error)))

    fun event(event: ApiEvent) = push(WsServerMessage.Event(event))

    fun raw(text: String) {
        toClient.trySend(text)
    }

    /** PC が接続を閉じる (失効・停止)。 */
    fun drop() {
        toClient.close()
    }

    private fun push(message: WsServerMessage) {
        toClient.trySend(ApiJson.encodeToString(WsServerMessage.Serializer, message))
    }
}
