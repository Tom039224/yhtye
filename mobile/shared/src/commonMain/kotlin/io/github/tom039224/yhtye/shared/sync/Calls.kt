package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.ApiJson
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.client.ApiException
import io.github.tom039224.yhtye.shared.client.CoreConnection
import io.github.tom039224.yhtye.shared.client.RpcException
import io.github.tom039224.yhtye.shared.client.UnexpectedReplyException
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/** [command] を実行し、[T] の応答を返す。違う種類の応答は [UnexpectedReplyException]、失敗は [RpcException]。 */
internal suspend inline fun <reified T : ApiResponse> CoreConnection.expect(command: ApiCommand, timeoutMs: Long? = null): T {
    val reply = if (timeoutMs == null) call(command) else call(command, timeoutMs)
    return reply as? T ?: throw UnexpectedReplyException(command, reply)
}

/** コマンドの `type` (エラーの文に使う。例: `send_user_message`)。 */
internal fun commandName(command: ApiCommand): String {
    val json = runCatching { ApiJson.encodeToJsonElement(ApiCommand.Serializer, command) }.getOrNull()
    return ((json as? JsonObject)?.get("type") as? JsonPrimitive)?.content ?: command::class.simpleName.orEmpty()
}

/** 画面に出す失敗の文 (コアのエラーはその `message`)。 */
fun describe(e: RpcException): String = when (e) {
    is ApiException -> e.error.message
    else -> e.message.orEmpty()
}
