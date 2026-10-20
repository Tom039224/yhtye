package io.github.tom039224.yhtye.ui.panels

import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.input.pointer.util.VelocityTracker
import io.github.tom039224.yhtye.ui.common.SwipeThresholds
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlin.math.abs

// レールの展開 (PC のホバーで広がるオーバーレイのタッチ版、Issue #18)。左パネルが開いているとき、同じ向き (左→右) にもう 1 回
// スワイプするとレールが広がってプロジェクトの名前が出る。逆の向き (右→左) で畳み、もう 1 回でパネルが閉じる (今までどおり
// PanelShell)。プロジェクトを選んでも畳む。

/** 広がる・畳むアニメーション (PC の `width .14s cubic-bezier(.3,0,.2,1)`)。 */
private val RAIL_EASING = CubicBezierEasing(0.3f, 0f, 0.2f, 1f)
private const val RAIL_SETTLE_MS = 140

/** レールの広がり具合。[progress] は 0 (畳んだ) … 1 (広がった) で指に付いて動き、[open] は落ち着く先。 */
@Stable
internal class RailExpansion(private val scope: CoroutineScope) {
    var progress: Float by mutableFloatStateOf(0f)
        internal set

    var open: Boolean by mutableStateOf(false)
        private set

    private var animation: Job? = null

    fun expand() = settle(true)

    fun collapse() = settle(false)

    internal fun stop() {
        animation?.cancel()
        animation = null
    }

    /** [to] へ動かす。[velocity] は progress/s (指を離したときの速さ)。 */
    internal fun settle(to: Boolean, velocity: Float = 0f) {
        stop()
        open = to
        val target = if (to) 1f else 0f
        val from = progress
        if (from == target) return
        animation = scope.launch {
            animate(from, target, velocity, tween(RAIL_SETTLE_MS, easing = RAIL_EASING)) { value, _ -> progress = value }
        }
    }
}

@Composable
internal fun rememberRailExpansion(): RailExpansion {
    val scope = rememberCoroutineScope()
    return remember(scope) { RailExpansion(scope) }
}

/**
 * 左パネルの横のスワイプのうち、レールのものだけを取る: 畳んでいれば左→右 (広げる)、広がっていれば右→左 (畳む)。
 * 逆の向き・縦の動き・子 (横にスクロールする要素) が先に取った動きは取らない (外の PanelShell がパネルを開閉する)。
 * 向きが決まった最初の動きを消費するので、外の PanelShell はその指でパネルを動かさない (子は Main パスで親より先に見る)。
 * 離したときは、速ければその向き、そうでなければ広がり具合が [SwipeThresholds.PANEL_COMMIT_SHARE] を超えて変わっていれば
 * その向きへ (パネルと同じ閾値)。
 *
 * @param distance 畳んだ幅から広がった幅までの道のり (px)。
 * @param edgeStart 画面の左端のこの幅から始まったタッチは扱わない (Android の「戻る」。PanelShell と同じ)。
 */
internal suspend fun PointerInputScope.railSwipe(
    expansion: RailExpansion,
    distance: Float,
    fling: Float,
    edgeStart: Float,
    haptic: HapticFeedback,
) = awaitEachGesture {
    // タップを取る子 (ボタン) は押下を消費するので、消費済みでも受ける
    val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Main)
    if (distance <= 0f || down.position.x < edgeStart) return@awaitEachGesture
    val opening = !expansion.open
    val slop = viewConfiguration.touchSlop
    val tracker = VelocityTracker()
    tracker.addPosition(down.uptimeMillis, down.position)
    var start: Offset? = null
    while (start == null) {
        val change = awaitPointerEvent(PointerEventPass.Main).changes.firstOrNull { it.id == down.id } ?: return@awaitEachGesture
        if (!change.pressed || change.isConsumed) return@awaitEachGesture
        tracker.addPosition(change.uptimeMillis, change.position)
        val moved = change.position - down.position
        if (moved.getDistance() < slop) continue
        if (abs(moved.x) <= abs(moved.y) || (moved.x > 0f) != opening) return@awaitEachGesture
        change.consume()
        start = change.position // スロップ分を飛ばさない (指の下から動き始める)
    }
    expansion.stop()
    val base = expansion.progress
    var cancelled = false
    while (true) {
        val change = awaitPointerEvent(PointerEventPass.Initial).changes.firstOrNull { it.id == down.id }
        if (change == null) {
            cancelled = true
            break
        }
        tracker.addPosition(change.uptimeMillis, change.position)
        if (!change.pressed) {
            change.consume()
            break
        }
        if (change.positionChange() != Offset.Zero) change.consume()
        expansion.progress = (base + (change.position.x - start.x) / distance).coerceIn(0f, 1f)
    }
    val velocity = if (cancelled) 0f else tracker.calculateVelocity().x
    val moved = expansion.progress - base
    val open = when {
        velocity > fling -> true
        velocity < -fling -> false
        moved > SwipeThresholds.PANEL_COMMIT_SHARE -> true
        moved < -SwipeThresholds.PANEL_COMMIT_SHARE -> false
        else -> expansion.open
    }
    if (open != expansion.open) haptic.performHapticFeedback(HapticFeedbackType.GestureEnd)
    expansion.settle(open, velocity / distance)
}
