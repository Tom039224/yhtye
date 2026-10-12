package io.github.tom039224.yhtye.shared.client

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow

/**
 * コアへの接続 (同期層 `sync.AppStore` が使う窓口)。実物は [YhtyeClient]、テストは偽のコア。
 */
interface CoreConnection {
    /** 接続状態。[ConnectionState.Connected.session] が変わったら切れていた間のイベントは届いていない。 */
    val state: StateFlow<ConnectionState>

    /** コアが流すイベント。取りこぼし得る (`seq` の飛びと `resync` で分かる)。 */
    val events: Flow<ApiEvent>

    /** [command] を既定の待ち時間で実行する。失敗は [RpcException]。 */
    suspend fun call(command: ApiCommand): ApiResponse

    /** [command] を実行し、[timeoutMs] まで応答を待つ。失敗は [RpcException]。 */
    suspend fun call(command: ApiCommand, timeoutMs: Long): ApiResponse
}
