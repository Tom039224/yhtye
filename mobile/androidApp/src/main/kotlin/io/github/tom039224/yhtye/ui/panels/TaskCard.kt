package io.github.tom039224.yhtye.ui.panels

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerInputScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.State
import io.github.tom039224.yhtye.shared.api.generated.Task
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus
import io.github.tom039224.yhtye.shared.sync.LogLine
import io.github.tom039224.yhtye.shared.sync.ProjectView
import io.github.tom039224.yhtye.shared.sync.Tone
import io.github.tom039224.yhtye.shared.sync.formatElapsed
import io.github.tom039224.yhtye.shared.sync.isActiveTone
import io.github.tom039224.yhtye.shared.sync.isTerminal
import io.github.tom039224.yhtye.shared.sync.openHelps
import io.github.tom039224.yhtye.shared.sync.taskAgent
import io.github.tom039224.yhtye.shared.sync.taskElapsedMs
import io.github.tom039224.yhtye.shared.sync.taskLog
import io.github.tom039224.yhtye.shared.sync.taskMeta
import io.github.tom039224.yhtye.shared.sync.taskProgress
import io.github.tom039224.yhtye.shared.sync.taskTone
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.conversation.ToolStatusIcon
import io.github.tom039224.yhtye.ui.tasks.AgentLabel
import io.github.tom039224.yhtye.ui.tasks.HelpNote
import io.github.tom039224.yhtye.ui.tasks.INTERRUPTED_NOTE
import io.github.tom039224.yhtye.ui.tasks.MetaLine
import io.github.tom039224.yhtye.ui.tasks.StepsRow
import io.github.tom039224.yhtye.ui.tasks.TaskStatusChip
import io.github.tom039224.yhtye.ui.tasks.metaIcon
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.abs

// 右パネルのタスクカード (PC の TaskCard、モックの `.task`)。右→左にスワイプするとカードが指に付いてずれ、下から `@` が
// 現れる。88dp を超えて離す (か 40dp 以上で速くはじく) とメンション (PC の `@` と同じ)、足りなければ戻る。

private val CARD_SHAPE = RoundedCornerShape(12.dp)

/** これを超えて離すとメンション (モックの ROW_ARM 88px)。 */
private val ARM = 88.dp

/** この先は指に付いて行かず重くなる (モックの ROW_MAX 132px とゴムの 0.25)。 */
private val MAX_PULL = 132.dp
private const val RUBBER = 0.25f

/** 短くてもこの速さではじけばメンション (0.45px/ms、40px 以上)。 */
private const val FLING_DP_PER_SECOND = 450f
private val FLING_MIN = 40.dp

/** 下の地が見えきるまでの距離 (モックの 36px)。 */
private val UNDER_FADE = 36.dp

private val SETTLE_EASING = CubicBezierEasing(0.2f, 0.8f, 0.2f, 1f)
private const val SETTLE_MS = 240
private const val FIRE_RETURN_DELAY_MS = 120L
private const val FIRE_MENTION_DELAY_MS = 300L
private const val FIRE_MS = 340
private const val SHEEN_MS = 1_900

/** カードのずれ (px。0 か負) と `@` の様子。 */
@Stable
private class RowSwipe(private val scope: CoroutineScope) {
    var dx by mutableFloatStateOf(0f)
    var armed by mutableStateOf(false)
    var fired by mutableStateOf(false)
    private var job: Job? = null

    fun stop() {
        job?.cancel()
        job = null
    }

    fun settleBack(velocity: Float = 0f) {
        stop()
        val from = dx
        job = scope.launch {
            animate(from, 0f, velocity, tween(SETTLE_MS, easing = SETTLE_EASING)) { v, _ -> dx = v }
            armed = false
        }
    }

    /** 確定: `@` が弾んでからカードが戻り、メンションを積む。 */
    fun fire(onMention: () -> Unit) {
        stop()
        armed = true
        fired = true
        job = scope.launch {
            delay(FIRE_RETURN_DELAY_MS)
            val from = dx
            launch { animate(from, 0f, 0f, tween(SETTLE_MS, easing = SETTLE_EASING)) { v, _ -> dx = v } }
            delay(FIRE_MENTION_DELAY_MS - FIRE_RETURN_DELAY_MS)
            onMention()
            delay(SETTLE_MS.toLong())
            armed = false
            fired = false
        }
    }
}

/**
 * タスクのカード。タップでタスク詳細、`@` でオーケストレータへ引用、■ で中止 (終わったタスクには無い)。
 *
 * @param gestureInsets 画面の端 (Android の「戻る」)。そこから始まった指はスワイプにしない。
 */
@Composable
fun TaskCard(
    task: Task,
    state: State,
    view: ProjectView,
    now: Long,
    gestureInsets: WindowInsets,
    onOpen: () -> Unit,
    onMention: () -> Unit,
    onCancel: () -> Unit,
) {
    val c = Yhtye.colors
    val tone = taskTone(task)
    val scope = rememberCoroutineScope()
    val swipe = remember(scope) { RowSwipe(scope) }
    val haptic = LocalHapticFeedback.current
    val density = LocalDensity.current
    val direction = LocalLayoutDirection.current
    val windowWidth = LocalWindowInfo.current.containerSize.width.toFloat()
    val edgeEnd = gestureInsets.getRight(density, direction).toFloat()
    var leftInRoot by remember { mutableFloatStateOf(0f) }
    val mention by rememberUpdatedState(onMention)
    val px = with(density) { SwipePx(ARM.toPx(), MAX_PULL.toPx(), FLING_DP_PER_SECOND.dp.toPx(), FLING_MIN.toPx()) }
    val handling = tone == Tone.Handling
    val border = if (handling) c.handCardLine else c.lineCard
    Box(
        Modifier
            .fillMaxWidth()
            .padding(bottom = 8.dp)
            .clip(CARD_SHAPE)
            .border(1.dp, border, CARD_SHAPE)
            .onGloballyPositioned { leftInRoot = it.positionInRoot().x }
            .pointerInput(swipe, px, edgeEnd, windowWidth) {
                mentionSwipe(swipe, px, haptic, isAtEdge = { x -> leftInRoot + x > windowWidth - edgeEnd }) {
                    haptic.performHapticFeedback(HapticFeedbackType.Confirm)
                    swipe.fire { mention() }
                }
            }
            .semantics(mergeDescendants = false) {
                contentDescription = "task ${task.id}"
                customActions = listOf(CustomAccessibilityAction("オーケストレータへ引用") { onMention(); true })
            },
    ) {
        MentionUnder(swipe, Modifier.matchParentSize())
        CardBody(task, state, view, now, tone, onOpen, onMention, onCancel, Modifier.graphicsLayer { translationX = swipe.dx })
    }
}

private data class SwipePx(val arm: Float, val max: Float, val fling: Float, val flingMin: Float)

/** カードをずらしたとき下に見える `@` (モックの `.under`)。 */
@Composable
private fun MentionUnder(swipe: RowSwipe, modifier: Modifier) {
    val c = Yhtye.colors
    val density = LocalDensity.current
    val fade = with(density) { UNDER_FADE.toPx() }
    val scale = remember { Animatable(0.8f) }
    val alpha = remember { Animatable(1f) }
    val armed = swipe.armed
    val fired = swipe.fired
    LaunchedEffect(armed, fired) {
        when {
            fired -> {
                scale.animateTo(1.45f, tween(FIRE_MS * 45 / 100))
                launch { alpha.animateTo(0.2f, tween(FIRE_MS * 55 / 100)) }
                scale.animateTo(1.1f, tween(FIRE_MS * 55 / 100))
            }
            armed -> {
                alpha.snapTo(1f)
                scale.animateTo(1.18f, spring(dampingRatio = 0.45f, stiffness = 900f))
            }
            else -> {
                alpha.snapTo(1f)
                scale.animateTo(0.8f, tween(180))
            }
        }
    }
    val bg by animateColorAsState(if (armed) c.accent else c.accentSoft, label = "at-bg")
    val fg by animateColorAsState(if (armed) c.onAccent else c.accent, label = "at-fg")
    Box(
        modifier
            .graphicsLayer { this.alpha = (-swipe.dx / fade).coerceIn(0f, 1f) }
            .background(c.mentionUnder)
            .padding(end = 18.dp),
        contentAlignment = Alignment.CenterEnd,
    ) {
        Box(
            Modifier
                .size(44.dp)
                .graphicsLayer {
                    scaleX = scale.value
                    scaleY = scale.value
                    this.alpha = alpha.value
                }
                .background(bg, CircleShape),
            contentAlignment = Alignment.Center,
        ) { YIcon(YIcons.At, fg, 20.dp) }
    }
}

/**
 * 右→左のドラッグだけを取る (左→右・縦は取らないので、パネルを閉じるドラッグと縦のスクロールになる)。
 * 親 (パネル) より先に Main パスで見て、取ったら消費するので親は手を引く。
 */
private suspend fun PointerInputScope.mentionSwipe(
    swipe: RowSwipe,
    px: SwipePx,
    haptic: HapticFeedback,
    isAtEdge: (Float) -> Boolean,
    onConfirm: () -> Unit,
) = awaitEachGesture {
    val down = awaitFirstDown(requireUnconsumed = false)
    if (swipe.fired || isAtEdge(down.position.x)) return@awaitEachGesture
    val slop = viewConfiguration.touchSlop
    val tracker = VelocityTracker()
    tracker.addPosition(down.uptimeMillis, down.position)
    var start: Offset? = null
    while (start == null) {
        val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: return@awaitEachGesture
        if (!change.pressed || change.isConsumed) return@awaitEachGesture
        tracker.addPosition(change.uptimeMillis, change.position)
        val moved = change.position - down.position
        if (moved.getDistance() < slop) continue
        if (abs(moved.x) <= abs(moved.y) || moved.x > 0f) return@awaitEachGesture
        change.consume()
        start = change.position // スロップ分を飛ばさない (指の下から動き始める)
    }
    swipe.stop()
    var cancelled = false
    while (true) {
        val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id }
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
        var dx = minOf(0f, change.position.x - start.x)
        if (dx < -px.max) dx = -px.max - (-dx - px.max) * RUBBER
        swipe.dx = dx
        val armed = -dx >= px.arm
        if (armed && !swipe.armed) haptic.performHapticFeedback(HapticFeedbackType.GestureThresholdActivate)
        swipe.armed = armed
    }
    val velocity = if (cancelled) 0f else tracker.calculateVelocity().x
    val pulled = -swipe.dx
    if (!cancelled && (pulled >= px.arm || (velocity < -px.fling && pulled >= px.flingMin))) {
        onConfirm()
    } else {
        swipe.settleBack()
    }
}

@Composable
private fun CardBody(
    task: Task,
    state: State,
    view: ProjectView,
    now: Long,
    tone: Tone,
    onOpen: () -> Unit,
    onMention: () -> Unit,
    onCancel: () -> Unit,
    modifier: Modifier,
) {
    val c = Yhtye.colors
    val handling = tone == Tone.Handling
    val finished = tone == Tone.Done || tone == Tone.Cancelled
    val bg = when {
        handling -> c.handCardBg
        finished -> c.bgInset
        else -> c.bgCard
    }
    val idColor = if (handling) c.handCardMeta else c.tx3
    val iconColor = if (handling) c.handCardIcon else c.tx3
    val agent = taskAgent(view, task)
    val elapsed = taskElapsedMs(view, task, now)?.let(::formatElapsed)
    val log = if (isTerminal(task)) emptyList() else taskLog(view, task.id)
    val meta = taskMeta(state, task)
    Column(
        modifier
            .fillMaxWidth()
            .background(bg)
            .clickable(role = Role.Button, onClickLabel = "エージェントの出力を見る", onClick = onOpen)
            .padding(start = 12.dp, end = 6.dp, top = 8.dp, bottom = 9.dp)
            .testTag("task-${task.id}"),
        verticalArrangement = Arrangement.spacedBy(7.dp),
    ) {
        Row(Modifier.fillMaxWidth().heightIn(min = 40.dp), verticalAlignment = Alignment.CenterVertically) {
            TaskStatusChip(task, tone)
            Text(task.id, color = idColor, fontFamily = MonoFont, fontSize = 11.5.sp, modifier = Modifier.padding(start = 8.dp))
            Spacer(Modifier.weight(1f))
            IconButton(
                onClick = onMention,
                modifier = Modifier.size(40.dp).semantics { contentDescription = "${task.id} をオーケストレータへ引用" }.testTag("mention-button-${task.id}"),
            ) { YIcon(YIcons.At, iconColor, 16.dp) }
            if (!isTerminal(task)) {
                IconButton(
                    onClick = onCancel,
                    modifier = Modifier.size(40.dp).semantics { contentDescription = "${task.id} を中止 (エージェントを停止)" }.testTag("cancel-task-${task.id}"),
                ) { YIcon(YIcons.Stop, iconColor, 12.dp) }
            }
        }
        Text(
            task.title,
            color = when {
                handling -> c.handCardTitle
                finished -> c.tx4
                else -> c.tx
            },
            fontSize = 14.5.sp,
            lineHeight = 21.sp,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(end = 8.dp),
        )
        if (isActiveTone(tone)) ProgressBar(taskProgress(task).toFloat(), tone)
        if (task.status == TaskStatus.Interrupted) {
            Row(horizontalArrangement = Arrangement.spacedBy(7.dp), modifier = Modifier.padding(end = 6.dp)) {
                YIcon(YIcons.Refresh, iconColor, 14.dp, Modifier.padding(top = 3.dp))
                Text(INTERRUPTED_NOTE, color = if (handling) c.handCardTx else c.tx2, fontSize = 13.5.sp, lineHeight = 21.sp)
            }
        }
        openHelps(state, task.id).forEach { HelpNote(it, c.handCardTx, c.handCardIcon, Modifier.padding(end = 6.dp)) }
        task.cancelReason?.let { MetaLine(YIcons.Ban, it, if (handling) c.handCardMetaStrong else c.tx4, description = "中止した理由: $it") }
        if (log.isNotEmpty()) LogLines(log, if (handling) c.handCardMeta else c.tx4)
        Row(Modifier.fillMaxWidth().padding(end = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            StepsRow(task, tone, Modifier.weight(1f))
            if (agent != null) AgentLabel(agent, elapsed, if (handling) c.handCardMeta else c.tx4)
        }
        meta?.let { MetaLine(metaIcon(it.icon), it.text, if (handling) c.handCardMetaStrong else c.tx4, description = it.title ?: it.text) }
    }
}

/** エージェントの最近の活動 (多くて 2 行)。 */
@Composable
private fun LogLines(log: List<LogLine>, color: Color) {
    Column(Modifier.padding(end = 6.dp).testTag("task-log"), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        log.forEach { l ->
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                YIcon(
                    when (l.kind) {
                        LogLine.Kind.Message -> YIcons.Message
                        LogLine.Kind.Tool -> YIcons.Wrench
                        LogLine.Kind.Error -> YIcons.Alert
                    },
                    color,
                    12.dp,
                )
                Text(
                    l.text,
                    color = color,
                    fontFamily = MonoFont,
                    fontSize = 11.5.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                if (l.kind == LogLine.Kind.Tool) ToolStatusIcon(l.status)
            }
        }
    }
}

/** 稼働中の進み具合 (細いバーと流れる光。モックの `.prog`)。 */
@Composable
private fun ProgressBar(share: Float, tone: Tone) {
    val c = Yhtye.colors
    val (bar, sheen) = if (tone == Tone.Reviewing) c.revBar to c.revSheen else c.implBar to c.implSheen
    val transition = rememberInfiniteTransition(label = "sheen")
    val at by transition.animateFloat(
        initialValue = -1.2f,
        targetValue = 3.2f,
        animationSpec = infiniteRepeatable(tween(SHEEN_MS, easing = LinearEasing), RepeatMode.Restart),
        label = "sheen",
    )
    BoxWithConstraints(
        Modifier
            .padding(end = 6.dp)
            .fillMaxWidth()
            .height(2.dp)
            .clip(RoundedCornerShape(2.dp))
            .background(c.lineCard)
            .semantics { contentDescription = "${(share * 100).toInt()}%" },
    ) {
        Box(Modifier.fillMaxWidth(share).fillMaxHeight().background(bar))
        val sheenWidth = maxWidth * 0.34f
        Box(
            Modifier
                .width(sheenWidth)
                .fillMaxHeight()
                .graphicsLayer { translationX = sheenWidth.toPx() * at }
                .background(Brush.horizontalGradient(listOf(Color.Transparent, sheen, Color.Transparent))),
        )
    }
}
