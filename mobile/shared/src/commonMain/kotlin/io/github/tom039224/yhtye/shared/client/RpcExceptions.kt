package io.github.tom039224.yhtye.shared.client

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiError
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse

/** [YhtyeClient.call] の失敗。 */
sealed class RpcException(message: String) : Exception(message)

/** PC がエラーで答えた。[error] の `message` は画面に出してよい。 */
class ApiException(val error: ApiError) : RpcException("${error.code.wire}: ${error.message}")

/** 時間内に応答が無かった (要求は PC で実行されているかもしれない)。 */
class RpcTimeoutException(command: ApiCommand, timeoutMs: Long) :
    RpcException("no reply to ${command::class.simpleName} within $timeoutMs ms")

/** 応答の前に接続が切れた (要求は PC で実行されたかもしれない。再送はしない)。 */
class ConnectionLostException : RpcException("the connection was lost before the reply")

/** 接続していないし、待ってもつながらない状態 ([state])。 */
class NotConnectedException(val state: ConnectionState) : RpcException("not connected: $state")

/** 要求に合わない種類の応答が返った (PC が新しい・古い)。 */
class UnexpectedReplyException(command: ApiCommand, reply: ApiResponse) :
    RpcException("unexpected reply to ${command::class.simpleName}: ${reply::class.simpleName}")
