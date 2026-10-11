package io.github.tom039224.yhtye.ui.shell

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.ui.common.SwipeThresholds
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlin.math.abs

// 中央の上に積む全画面 (タスク詳細・設定。モックの `.screen`)。右から入り、左→右のスワイプ (指に 1:1 で付いてずれる) か
// 戻る (予測型を含む) で閉じる。積むのは 1 枚だけ。

/** 開閉のアニメーション (モックの `.screen` の transition .26s)。 */
private val SCREEN_EASING = CubicBezierEasing(0.2f, 0.8f, 0.2f, 1f)
private const val OPEN_MS = 260
private const val RETURN_MS = 220
private const val MIN_CLOSE_MS = 140
private const val MAX_CLOSE_MS = 280

/** 速さが足りないときに使う速さ (px/ms。モックの 1.2)。 */
private const val SLOW_PX_PER_MS = 1.2f
private const val MILLIS_PER_SECOND = 1_000f
private val SCREEN_SHADOW = 24.dp

/** 画面のずれ (px。0 = 開いている、幅 = 閉じている)。 */
@Stable
private class ScreenMotion(private val scope: CoroutineScope) {
    var offset by mutableFloatStateOf(Float.MAX_VALUE)
    var width = 0f
    private var job: Job? = null

    /** 次の閉じるアニメーションの初速 (px/s。指を離したときの速さ)。 */
    var releaseVelocity = 0f

    fun stop() {
        job?.cancel()
        job = null
    }

    fun animateTo(target: Float, durationMs: Int, velocity: Float = 0f, then: () -> Unit = {}) {
        stop()
        val from = offset
        job = scope.launch {
            animate(from, target, velocity, tween(durationMs, easing = SCREEN_EASING)) { value, _ -> offset = value }
            then()
        }
    }
}

/**
 * [screen] を中央の上に積む (null: 何も積まない)。閉じる操作は [onClose] を呼ぶだけで、呼ぶ側が [screen] を null に
 * すると右へ抜けて消える (それまでは前の画面を出し続ける)。
 *
 * - 左→右のスワイプ: 画面が指に付いてずれ、幅の [SwipeThresholds.DETAIL_CLOSE_SHARE] を超えるか速く離すと閉じ、
 *   足りなければ戻る。右→左は何もしない。
 * - 画面の左右端 ([gestureInsets]) から始まったタッチは扱わない (Android の「戻る」)。
 * - 子が先に横の動きを取ったら (はみ出したセッションのタブ・コードブロック) その指は扱わない。
 * - 戻る (予測型を含む) は指に合わせて画面がずれ、確定で閉じる。後から合成されるので、パネルの「戻る」より先に効く。
 */
@Composable
fun <T : Any> ScreenStack(
    screen: T?,
    onClose: () -> Unit,
    gestureInsets: WindowInsets,
    content: @Composable (T) -> Unit,
) {
    val scope = rememberCoroutineScope()
    val motion = remember(scope) { ScreenMotion(scope) }
    var shown by remember { mutableStateOf(screen) }
    val close by rememberUpdatedState(onClose)
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val width = constraints.maxWidth.toFloat()
        motion.width = width
        LaunchedEffect(screen) {
            if (screen != null) {
                if (shown == null || motion.offset >= width) motion.offset = width
                shown = screen
                motion.animateTo(0f, OPEN_MS)
            } else if (shown != null) {
                val v = motion.releaseVelocity
                motion.releaseVelocity = 0f
                val duration = if (v > 0f) {
                    ((width - motion.offset) / maxOf(v / MILLIS_PER_SECOND, SLOW_PX_PER_MS)).toInt().coerceIn(MIN_CLOSE_MS, MAX_CLOSE_MS)
                } else {
                    OPEN_MS
                }
                motion.animateTo(width, duration, v) { shown = null }
            }
        }
        val current = shown ?: return@BoxWithConstraints
        val share = if (width > 0f) (1f - motion.offset / width).coerceIn(0f, 1f) else 0f
        Box(Modifier.fillMaxSize().graphicsLayer { alpha = share }.background(Yhtye.colors.sheetScrim))
        PredictiveBackHandler(enabled = screen != null) { progress ->
            motion.stop()
            try {
                progress.collect { event -> motion.offset = motion.width * event.progress }
                close()
            } catch (e: CancellationException) {
                motion.animateTo(0f, RETURN_MS)
                throw e
            }
        }
        val density = LocalDensity.current
        val direction = LocalLayoutDirection.current
        val edgeStart = gestureInsets.getLeft(density, direction).toFloat()
        val edgeEnd = gestureInsets.getRight(density, direction).toFloat()
        val fling = with(density) { SwipeThresholds.FLING_DP_PER_SECOND.dp.toPx() }
        Box(
            Modifier
                .fillMaxSize()
                // ずらす層より前 (外) に置く: 中に置くと、画面がずれた分だけ指の座標も戻り、指の半分しか付いて来ない (T-74)
                .pointerInput(motion, edgeStart, edgeEnd, fling) {
                    swipeToClose(motion, edgeStart, edgeEnd, fling) { close() }
                }
                .graphicsLayer {
                    translationX = motion.offset.coerceAtMost(width)
                    shadowElevation = SCREEN_SHADOW.toPx()
                }
                .background(Yhtye.colors.bgCenter)
                .testTag("stacked-screen"),
        ) { content(current) }
    }
}

/** 左→右のドラッグで画面をずらし、離した位置と速さで閉じるか戻すかを決める。 */
private suspend fun PointerInputScope.swipeToClose(
    motion: ScreenMotion,
    edgeStart: Float,
    edgeEnd: Float,
    fling: Float,
    onClose: () -> Unit,
) = awaitEachGesture {
    // タップを取る子 (ボタン) は押下を消費するので、消費済みでも受ける
    val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Main)
    val x = down.position.x
    if (x < edgeStart || x > size.width - edgeEnd) return@awaitEachGesture
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
        // 縦はスクロールのまま、右→左は何もしない
        if (abs(moved.x) <= abs(moved.y) || moved.x < 0f) return@awaitEachGesture
        change.consume()
        start = change.position
    }
    motion.stop()
    // 戻る途中で掴み直したら、その位置から指に付いて行く
    val base = motion.offset.coerceIn(0f, motion.width)
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
        motion.offset = (base + change.position.x - start.x).coerceAtLeast(0f)
    }
    val velocity = if (cancelled) 0f else tracker.calculateVelocity().x
    if (motion.offset > motion.width * SwipeThresholds.DETAIL_CLOSE_SHARE || velocity > fling) {
        motion.releaseVelocity = maxOf(velocity, 0f)
        onClose()
    } else {
        motion.animateTo(0f, RETURN_MS)
    }
}
