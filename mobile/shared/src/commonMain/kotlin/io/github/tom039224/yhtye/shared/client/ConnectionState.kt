package io.github.tom039224.yhtye.shared.client

/** [YhtyeClient] の接続状態。 */
sealed interface ConnectionState {
    /** 動いていない ([YhtyeClient.start] 前・[YhtyeClient.stop] 後)。 */
    data object Idle : ConnectionState

    /** 接続を試している。[attempt] は連続した失敗の数 (初回は 0)。 */
    data class Connecting(val attempt: Int) : ConnectionState

    /**
     * 接続済み。[session] は接続するたびに増える番号: 値が変わったら (再接続したら) 切れていた間の
     * イベントは届いていないので、利用側は状態を取り直す。
     */
    data class Connected(val session: Long) : ConnectionState

    /** 切れた・つながらなかったので [delayMs] 後に再接続する。[reason] は直前の失敗の説明。 */
    data class WaitingToReconnect(val attempt: Int, val delayMs: Long, val reason: String?) : ConnectionState

    /** 401: 端末トークンが無効 (PC で失効された)。再接続しない。ペアリングし直す。 */
    data object Revoked : ConnectionState

    /**
     * 403: PC と同じ Tailscale ユーザーの端末ではない (か、識別できない) と拒否された。
     * 自動では再接続しない ([YhtyeClient.start] でやり直す)。
     */
    data class Rejected(val message: String?) : ConnectionState
}
