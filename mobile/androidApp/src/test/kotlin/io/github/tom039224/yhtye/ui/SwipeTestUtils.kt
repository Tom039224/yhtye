package io.github.tom039224.yhtye.ui

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.TouchInjectionScope
import androidx.compose.ui.test.swipe

/** 確定に届かないくらいゆっくり動かす速さ (dp/s)。フリックの閾値 (245〜315dp/s) よりずっと遅い。 */
const val SLOW_DP_PER_SECOND = 40f

/** タッチイベントの間隔 (ms。`swipe` の既定)。これより短い時間では動かせない。 */
private const val EVENT_PERIOD_MS = 10L

private const val MILLIS_PER_SECOND = 1_000f

/**
 * 指を [from] から横へ動かして離す。向きの判定 (タッチスロップ) を越えるまでの分は別にかかるので、越えてから [pulled] (px) だけ
 * 動くよう全体の長さを決める。[dpPerSecond] は動かす速さ (画面の密度に依らない。離す直前の速さがそのままフリックの速さになる)。
 */
fun TouchInjectionScope.dragFrom(from: Offset, pulled: Float, leftward: Boolean, dpPerSecond: Float = SLOW_DP_PER_SECOND) {
    val total = viewConfiguration.touchSlop + pulled
    val to = from + Offset(if (leftward) -total else total, 0f)
    val durationMs = (total / (dpPerSecond * density) * MILLIS_PER_SECOND).toLong().coerceAtLeast(EVENT_PERIOD_MS)
    swipe(from, to, durationMs)
}
