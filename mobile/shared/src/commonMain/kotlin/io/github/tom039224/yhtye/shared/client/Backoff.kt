package io.github.tom039224.yhtye.shared.client

/** 再接続の待ち時間: [initialMs] から倍々に、[maxMs] で頭打ち。 */
data class Backoff(
    val initialMs: Long = 1_000,
    val maxMs: Long = 30_000,
) {
    init {
        require(initialMs in 1..maxMs) { "bad backoff: $initialMs..$maxMs" }
    }

    /** [attempt] 回目 (1 から) の失敗の後に待つ時間。 */
    fun delayMs(attempt: Int): Long {
        var delay = initialMs
        repeat((attempt - 1).coerceAtLeast(0)) {
            if (delay >= maxMs) return maxMs
            delay *= 2
        }
        return delay.coerceAtMost(maxMs)
    }
}
