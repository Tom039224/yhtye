package io.github.tom039224.yhtye.ui.shell

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.systemGestures
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerInputChange
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.window.core.layout.WindowSizeClass
import androidx.window.core.layout.computeWindowSizeClass
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.ui.common.SwipeThresholds
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlin.math.abs

// Discord 型の 3 パネル (docs/planned/android-client.md「段階 3」のジェスチャ、モックの「パネルの位置」「ジェスチャ」)。
// 中央の画面が指に追従して横にずれ、下の左パネル/右パネルが見える。開いたとき中央を [PEEK] 残し、そこのタップで閉じる。
// 幅の広い端末 (WindowSizeClass の Expanded) では左右のパネルを常時表示し、ジェスチャは使わない。

/** パネルが開いたとき中央が見えている幅 (モックの `--peek`)。 */
val PEEK: Dp = 64.dp

/** 開閉のアニメーション (モックの `cubic-bezier(.2,.8,.2,1)`、120〜300ms)。 */
private val PANEL_EASING = CubicBezierEasing(0.2f, 0.8f, 0.2f, 1f)
private const val MIN_SETTLE_MS = 120
private const val MAX_SETTLE_MS = 300
private const val BUTTON_SETTLE_MS = 240

/** 速さが足りないときに使う速さ (px/ms。モックの 1.1)。 */
private const val SLOW_PX_PER_MS = 1.1f
private const val MILLIS_PER_SECOND = 1_000f

/** 常時表示にしたときの左右のパネルの幅。 */
private val WIDE_LEFT_WIDTH = 320.dp
private val WIDE_RIGHT_WIDTH = 360.dp

/** 中央をずらしたときの角の丸み・影 (モックの `#center.shifted`)。 */
private val SHIFTED_CORNER = 14.dp
private val SHIFTED_SHADOW = 16.dp

enum class Panel { Left, Right }

/**
 * パネルの開閉の状態。[open] は落ち着いた先 (またはアニメーションの行き先)、[offset] は中央のずれ (px。右が正 = 左パネル)。
 * ボタンからは [show] / [hide] (アニメーションは [scope] で動く)。
 */
@Stable
class PanelShellState internal constructor(initial: Panel?, private val scope: CoroutineScope) {
    var open: Panel? by mutableStateOf(initial)
        private set

    /** 中央のずれ (px)。 */
    var offset: Float by mutableFloatStateOf(0f)
        internal set

    /** 左右のパネルを常時表示している (幅の広い端末)。そのときは [show] / [hide] は何もしない。 */
    var wide: Boolean by mutableStateOf(false)
        internal set

    internal var panelWidth: Float by mutableFloatStateOf(0f)
    private var animation: Job? = null
    internal var initialized = false

    /** [panel] を開く (開いていれば何もしない)。 */
    fun show(panel: Panel) {
        if (wide) return
        settleTo(panel, velocity = 0f, durationMs = BUTTON_SETTLE_MS)
    }

    /** 開いているパネルを閉じて中央に戻る。 */
    fun hide() {
        if (wide) return
        settleTo(null, velocity = 0f, durationMs = BUTTON_SETTLE_MS)
    }

    internal fun positionOf(panel: Panel?): Float = when (panel) {
        Panel.Left -> panelWidth
        Panel.Right -> -panelWidth
        null -> 0f
    }

    internal fun stopAnimation() {
        animation?.cancel()
        animation = null
    }

    /** 幅が変わった・最初の配置: アニメーションせずに今のパネルの位置に置く。 */
    internal fun snapToOpen() {
        stopAnimation()
        offset = if (wide) 0f else positionOf(open)
    }

    /** [panel] へ動かす。[velocity] は px/s (アニメーションの初速)。 */
    internal fun settleTo(panel: Panel?, velocity: Float, durationMs: Int) {
        stopAnimation()
        open = panel
        val target = positionOf(panel)
        val from = offset
        if (from == target) return
        animation = scope.launch {
            animate(from, target, velocity, tween(durationMs, easing = PANEL_EASING)) { value, _ -> offset = value }
        }
    }

    internal companion object {
        fun durationFor(distance: Float, velocityPxPerSecond: Float): Int {
            val speed = maxOf(abs(velocityPxPerSecond) / MILLIS_PER_SECOND, SLOW_PX_PER_MS)
            return (distance / speed).toInt().coerceIn(MIN_SETTLE_MS, MAX_SETTLE_MS)
        }
    }
}

/** 開いているパネルは回転などの作り直しでも残す。 */
@Composable
fun rememberPanelShellState(): PanelShellState {
    var saved by rememberSaveable { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val state = remember(scope) { PanelShellState(saved?.let(Panel::valueOf), scope) }
    LaunchedEffect(state.open) { saved = state.open?.name }
    return state
}

/** 中身が知りたいパネルの状態 (中央のトップバーが ☰ を出すか、メンションを積んだら中央に戻るなど)。 */
val LocalPanelShell = staticCompositionLocalOf<PanelShellState?> { null }

/**
 * 3 パネルの骨格。[left] / [right] は狭い端末では中央の下に隠れていて、スワイプかボタンで見える。
 *
 * - 画面の左右端 ([gestureInsets]、既定は `WindowInsets.systemGestures`) から始まったタッチは扱わない (Android の「戻る」)。
 * - 子が先に横の動きを取ったら (横にはみ出したコードブロック・表の `horizontalScroll`、右パネルのタスクカードのスワイプ)
 *   その指はパネルを動かさない。縦のスクロールが先に動いたときも同じ。
 * - 最初のタッチスロップを越えた向きで決める。縦ならその指では横に切り替えない。
 * - 1 回のスワイプで動くのは今の位置から隣まで。離した位置と速さで開く/閉じるを決める ([SwipeThresholds])。
 * - 戻る (予測型を含む) は開いているパネル → 中央 の順に 1 段ずつ閉じる。
 */
@Composable
fun PanelShell(
    state: PanelShellState,
    left: @Composable () -> Unit,
    right: @Composable () -> Unit,
    center: @Composable () -> Unit,
    modifier: Modifier = Modifier,
    gestureInsets: WindowInsets = WindowInsets.systemGestures,
    /** 中央の上に積んだ画面 (タスク詳細・設定) が開いている間などはパネルを動かさない。 */
    swipeEnabled: Boolean = true,
    /** 右パネルを開けるか (履歴のビューには右パネルが無い。モックと同じ)。 */
    rightEnabled: Boolean = true,
) {
    BoxWithConstraints(modifier.fillMaxSize().background(Yhtye.colors.bgApp)) {
        val sizeClass = WindowSizeClass.BREAKPOINTS_V1.computeWindowSizeClass(maxWidth.value, maxHeight.value)
        val wide = sizeClass.isWidthAtLeastBreakpoint(WindowSizeClass.WIDTH_DP_EXPANDED_LOWER_BOUND)
        val density = LocalDensity.current
        val panelWidthPx = with(density) { (maxWidth - PEEK).toPx() }
        LaunchedEffect(wide, panelWidthPx) {
            state.wide = wide
            state.panelWidth = panelWidthPx
            state.snapToOpen()
            state.initialized = true
        }
        CompositionLocalProvider(LocalPanelShell provides state) {
            if (wide) {
                WideLayout(left, right, center)
            } else {
                NarrowLayout(state, left, right, center, gestureInsets, swipeEnabled, rightEnabled, panelWidthPx)
            }
        }
    }
}

@Composable
private fun WideLayout(left: @Composable () -> Unit, right: @Composable () -> Unit, center: @Composable () -> Unit) {
    val c = Yhtye.colors
    Row(Modifier.fillMaxSize()) {
        Box(Modifier.width(WIDE_LEFT_WIDTH).fillMaxHeight().background(c.bgPanel).testTag("panel-left")) { left() }
        VerticalDivider(color = c.line)
        Box(Modifier.weight(1f).fillMaxHeight().background(c.bgCenter).testTag("panel-center")) { center() }
        VerticalDivider(color = c.line)
        Box(Modifier.width(WIDE_RIGHT_WIDTH).fillMaxHeight().background(c.bgPanel).testTag("panel-right")) { right() }
    }
}

@Composable
private fun NarrowLayout(
    state: PanelShellState,
    left: @Composable () -> Unit,
    right: @Composable () -> Unit,
    center: @Composable () -> Unit,
    gestureInsets: WindowInsets,
    swipeEnabled: Boolean,
    rightEnabled: Boolean,
    panelWidthPx: Float,
) {
    val c = Yhtye.colors
    val density = LocalDensity.current
    val direction = LocalLayoutDirection.current
    val haptic = LocalHapticFeedback.current
    val edgeStart = gestureInsets.getLeft(density, direction).toFloat()
    val edgeEnd = gestureInsets.getRight(density, direction).toFloat()
    val fling = with(density) { SwipeThresholds.FLING_DP_PER_SECOND.dp.toPx() }
    val panelWidth = with(density) { panelWidthPx.toDp() }

    PredictiveBack(state)

    Box(
        Modifier
            .fillMaxSize()
            .then(
                if (swipeEnabled) {
                    Modifier.pointerInput(state, edgeStart, edgeEnd, fling, rightEnabled) {
                        panelDrag(state, edgeStart, edgeEnd, fling, rightEnabled, haptic)
                    }
                } else {
                    Modifier
                },
            ),
    ) {
        val offset = state.offset
        if (offset > 0f) {
            Box(Modifier.width(panelWidth).fillMaxHeight().background(c.bgPanel).testTag("panel-left")) { left() }
        }
        if (offset < 0f) {
            Box(
                Modifier.align(Alignment.TopEnd).width(panelWidth).fillMaxHeight().background(c.bgPanel).testTag("panel-right"),
            ) { right() }
        }
        val shifted = offset != 0f
        Box(
            Modifier
                .fillMaxSize()
                .graphicsLayer {
                    translationX = state.offset
                    if (shifted) {
                        shape = RoundedCornerShape(SHIFTED_CORNER)
                        clip = true
                        shadowElevation = SHIFTED_SHADOW.toPx()
                    }
                }
                .background(c.bgCenter)
                .testTag("panel-center"),
        ) {
            center()
            if (shifted) {
                // 開いているパネルの横に残った中央: 中身を触らせず、タップで閉じる (モックの #shield)
                val share = if (panelWidthPx > 0f) minOf(1f, abs(offset) / panelWidthPx) else 1f
                Box(
                    Modifier
                        .fillMaxSize()
                        .graphicsLayer { alpha = share }
                        .background(c.scrim)
                        .testTag("panel-shield")
                        .pointerInput(state) { detectTapGestures { state.hide() } },
                )
            }
        }
    }
}

/** 戻る: 開いているパネルを閉じる。予測型の戻るでは指に合わせて中央が戻り、取り消したら元の位置に戻る。 */
@Composable
private fun PredictiveBack(state: PanelShellState) {
    PredictiveBackHandler(enabled = !state.wide && state.open != null) { progress ->
        state.stopAnimation()
        val start = state.offset
        try {
            progress.collect { event -> state.offset = start * (1f - event.progress) }
            state.settleTo(null, velocity = 0f, durationMs = BUTTON_SETTLE_MS)
        } catch (e: CancellationException) {
            state.settleTo(state.open, velocity = 0f, durationMs = BUTTON_SETTLE_MS)
            throw e
        }
    }
}

/**
 * パネルを動かす指の処理。子 (コードブロックの横スクロール・縦のリスト) が先に動きを取ったら手を引く
 * (子は Main パスで親より先にイベントを見る)。横に決まった後は Initial パスで先に取り、子に渡さない。
 */
private suspend fun androidx.compose.ui.input.pointer.PointerInputScope.panelDrag(
    state: PanelShellState,
    edgeStart: Float,
    edgeEnd: Float,
    fling: Float,
    rightEnabled: Boolean,
    haptic: HapticFeedback,
) = awaitEachGesture {
    // タップを取る子 (ボタン) は押下を消費するので、消費済みでも受ける
    val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Main)
    if (state.wide || !state.initialized) return@awaitEachGesture
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
        if (abs(moved.x) <= abs(moved.y)) return@awaitEachGesture
        change.consume()
        start = change.position // スロップ分を飛ばさない (指の下から動き始める)
    }
    state.stopAnimation()
    val base = state.positionOf(state.open)
    val lo = if (base > 0f || !rightEnabled) 0f else -state.panelWidth
    val hi = if (base < 0f) 0f else state.panelWidth
    var cancelled = false
    while (true) {
        val change: PointerInputChange? = awaitPointerEvent(PointerEventPass.Initial).changes.firstOrNull { it.id == down.id }
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
        state.offset = (base + change.position.x - start.x).coerceIn(lo, hi)
    }
    val velocity = if (cancelled) 0f else tracker.calculateVelocity().x
    settleAfterDrag(state, base, lo, hi, velocity, fling, haptic)
}

/**
 * 離したとき: 速ければその向きの隣へ、そうでなければ指を動かした距離が [SwipeThresholds.PANEL_COMMIT_SHARE] を超えていれば
 * ドラッグを始めた位置 ([base]) の隣へ、足りなければ [base] に戻る。開閉が変わったら軽く震わせる。
 */
private fun settleAfterDrag(
    state: PanelShellState,
    base: Float,
    lo: Float,
    hi: Float,
    velocity: Float,
    fling: Float,
    haptic: HapticFeedback,
) {
    val pos = state.offset
    val snaps = listOf(-state.panelWidth, 0f, state.panelWidth).filter { it in lo..hi }
    // 右パネルを開いたまま右パネルが無くなったときなどは、範囲内の最寄りから始めたことにする
    val anchor = base.coerceIn(lo, hi)
    val commit = state.panelWidth * SwipeThresholds.PANEL_COMMIT_SHARE
    val target = when {
        velocity > fling -> snaps.firstOrNull { it > pos + 0.5f } ?: snaps.last()
        velocity < -fling -> snaps.lastOrNull { it < pos - 0.5f } ?: snaps.first()
        pos - anchor > commit -> snaps.firstOrNull { it > anchor + 0.5f } ?: anchor
        pos - anchor < -commit -> snaps.lastOrNull { it < anchor - 0.5f } ?: anchor
        else -> anchor
    }
    val panel = when {
        target > 0f -> Panel.Left
        target < 0f -> Panel.Right
        else -> null
    }
    if (panel != state.open) haptic.performHapticFeedback(HapticFeedbackType.GestureEnd)
    state.settleTo(panel, velocity, PanelShellState.durationFor(abs(target - pos), velocity))
}
