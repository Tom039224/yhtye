package io.github.tom039224.yhtye.ui.conversation

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.Easing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.State
import io.github.tom039224.yhtye.shared.api.generated.TaskKind
import io.github.tom039224.yhtye.shared.api.generated.TextKind
import io.github.tom039224.yhtye.shared.sync.Tone
import io.github.tom039224.yhtye.shared.sync.TranscriptItem
import io.github.tom039224.yhtye.shared.sync.taskTone
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.common.breatheAlpha
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

// 会話の 1 行ずつ (PC の TranscriptView と同じ種類)。ユーザーの発言は吹き出し、オーケストレータの返事は Markdown、
// ツール呼び出し・Yhtye の操作・考えたことは控えめなモノスペースの 1 行 (Yhtye の操作と考えたことは折りたためる)、
// 受信箱の通知は破線の控えめな枠。

private val ROW_PADDING = 16.dp
private val BUBBLE_SHAPE = RoundedCornerShape(topStart = 18.dp, topEnd = 18.dp, bottomEnd = 6.dp, bottomStart = 18.dp)
private val FOLD_SHAPE = RoundedCornerShape(8.dp)
private val CARD_SHAPE = RoundedCornerShape(10.dp)
private const val BUBBLE_MAX_SHARE = 0.86f
private const val CARET_BLINK_MS = 1_000

/** 半分は点き、半分は消える (モックの `caret` の `steps(1)`)。 */
private val BLINK = Easing { if (it < 0.5f) 0f else 1f }

/** 1 行。[queued] は受信箱でまだ届いていない発言。 */
@Composable
fun TranscriptRow(item: TranscriptItem, queued: Set<Long>, state: State?) {
    val pad = Modifier.fillMaxWidth().padding(horizontal = ROW_PADDING, vertical = 4.dp)
    when (item) {
        is TranscriptItem.User -> UserBubble(item.text, item.inboxId in queued)
        is TranscriptItem.Text ->
            if (item.textKind == TextKind.Thought) {
                Box(pad) { ThoughtFold(item.text, streaming = false) }
            } else {
                SelectionContainer(pad) { Markdown(item.text) }
            }
        is TranscriptItem.Tool -> Box(pad) { ToolLine(item) }
        is TranscriptItem.YhtyeTool -> Box(pad) { YhtyeToolFold(item) }
        is TranscriptItem.Notice -> Box(pad) { NoticeBox(item) }
        is TranscriptItem.GroupCard -> Box(pad) { GroupCardView(item, state) }
        is TranscriptItem.Turn -> Box(pad) {
            MonoLine(
                if (item.error) YIcons.Alert else YIcons.Stop,
                "turn ended: ${item.outcome}",
                if (item.error) Yhtye.colors.handFg else Yhtye.colors.tx4,
            )
        }
        is TranscriptItem.Compact -> Box(pad) { MonoLine(YIcons.Minimize, "コンテキストを圧縮しました (/compact)", Yhtye.colors.tx4) }
        is TranscriptItem.Lifecycle -> Box(pad) {
            MonoLine(if (item.error) YIcons.Alert else YIcons.Info, item.text, if (item.error) Yhtye.colors.handFg else Yhtye.colors.tx4)
        }
        is TranscriptItem.Prompt -> Box(pad) { Fold(YIcons.FileText, "prompt ${formatTime(item.ts)}", item.text, prose = false) }
    }
}

/** ストリーミング途中の本文 (届くたびに描き直す。末尾にカーソル)。 */
@Composable
fun StreamingMessage(text: String) {
    Column(Modifier.fillMaxWidth().padding(horizontal = ROW_PADDING, vertical = 4.dp).testTag("streaming")) {
        if (text.isNotEmpty()) Markdown(text)
        Caret()
    }
}

/** ストリーミング途中の考え (PC の「thinking…」。アイコンが呼吸する)。 */
@Composable
fun StreamingThought(text: String) {
    Box(Modifier.fillMaxWidth().padding(horizontal = ROW_PADDING, vertical = 4.dp)) { ThoughtFold(text, streaming = true) }
}

@Composable
private fun Caret() {
    val transition = rememberInfiniteTransition(label = "caret")
    val alpha by transition.animateFloat(
        initialValue = 1f,
        targetValue = 0f,
        animationSpec = infiniteRepeatable(tween(CARET_BLINK_MS, easing = BLINK), RepeatMode.Restart),
        label = "caret",
    )
    Box(Modifier.padding(top = 4.dp).size(8.dp, 16.dp).alpha(alpha).background(Yhtye.colors.accent))
}

@Composable
private fun UserBubble(text: String, queued: Boolean) {
    val c = Yhtye.colors
    BoxWithConstraints(
        Modifier.fillMaxWidth().padding(start = ROW_PADDING, end = ROW_PADDING, top = 14.dp, bottom = 4.dp),
        contentAlignment = Alignment.CenterEnd,
    ) {
        Row(
            Modifier
                .widthIn(max = maxWidth * BUBBLE_MAX_SHARE)
                .clip(BUBBLE_SHAPE)
                .background(c.bubbleBg)
                .border(1.dp, c.bubbleLine, BUBBLE_SHAPE)
                .padding(horizontal = 13.dp, vertical = 9.dp)
                .testTag("user-bubble"),
            verticalAlignment = Alignment.Bottom,
        ) {
            SelectionContainer(Modifier.weight(1f, fill = false)) {
                Text(text, color = c.tx, fontSize = 15.sp, lineHeight = 24.sp)
            }
            if (queued) {
                Box(Modifier.padding(start = 6.dp).semantics { contentDescription = "待機中" }) {
                    YIcon(YIcons.Clock, c.tx3, size = 14.dp)
                }
            }
        }
    }
}

/** 控えめなモノスペースの 1 行 (ターンの終わり・起動と停止)。 */
@Composable
private fun MonoLine(icon: ImageVector, text: String, color: Color, trailing: (@Composable () -> Unit)? = null) {
    Row(Modifier.fillMaxWidth().heightIn(min = 30.dp), verticalAlignment = Alignment.CenterVertically) {
        YIcon(icon, Yhtye.colors.tx5, size = 14.dp)
        Text(
            text,
            color = color,
            fontFamily = MonoFont,
            fontSize = 12.sp,
            lineHeight = 17.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f).padding(start = 8.dp),
        )
        trailing?.invoke()
    }
}

@Composable
private fun ToolLine(item: TranscriptItem.Tool) {
    val c = Yhtye.colors
    val label = if (item.calls.isEmpty()) item.title else "${item.title} → ${item.calls.joinToString(", ")}"
    MonoLine(YIcons.Wrench, label, c.tx4) { ToolStatusIcon(item.status) }
}

/** ツール呼び出しの状態 (PC の ToolStatus)。 */
@Composable
private fun ToolStatusIcon(status: String?) {
    val c = Yhtye.colors
    val (icon, color, spin) = when (status) {
        "completed" -> Triple(YIcons.Check, c.doneBar, false)
        "failed" -> Triple(YIcons.X, c.handBar, false)
        "in_progress" -> Triple(YIcons.Loader, c.implBar, true)
        else -> Triple(YIcons.Clock, c.tx4, false)
    }
    Box(Modifier.padding(start = 6.dp).semantics { stateDescription = status ?: "pending" }) { YIcon(icon, color, 14.dp, spin = spin) }
}

@Composable
private fun YhtyeToolFold(item: TranscriptItem.YhtyeTool) {
    val c = Yhtye.colors
    Fold(
        icon = YIcons.Bolt,
        label = item.tool,
        body = item.detail,
        prose = false,
        color = if (item.ok) c.tx4 else c.handFg,
    ) {
        if (item.by == "user") YIcon(YIcons.User, c.tx4, 12.dp, Modifier.padding(start = 4.dp))
        Box(Modifier.padding(start = 6.dp).semantics { stateDescription = if (item.ok) "ok" else "error" }) {
            YIcon(if (item.ok) YIcons.Check else YIcons.Alert, if (item.ok) c.doneBar else c.handBar, 14.dp)
        }
    }
}

/** 考えたこと: PC と同じく閉じた折りたたみ (アイコンだけ。開くと文)。 */
@Composable
private fun ThoughtFold(text: String, streaming: Boolean) {
    val alpha = if (streaming) breatheAlpha() else 1f
    Fold(
        icon = YIcons.Thought,
        label = "",
        body = text,
        prose = true,
        iconAlpha = alpha,
        description = if (streaming) "考え中" else "思考",
    )
}

/** 折りたためる控えめな 1 行 (タップで開く)。 */
@Composable
private fun Fold(
    icon: ImageVector,
    label: String,
    body: String,
    prose: Boolean,
    color: Color = Yhtye.colors.tx4,
    iconAlpha: Float = 1f,
    description: String? = null,
    trailing: @Composable () -> Unit = {},
) {
    val c = Yhtye.colors
    var open by rememberSaveable { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth()) {
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 32.dp)
                .clickable(role = Role.Button, onClickLabel = if (open) "閉じる" else "開く") { open = !open }
                .then(if (description != null) Modifier.semantics { contentDescription = description } else Modifier)
                .testTag("fold"),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            YIcon(icon, c.tx5, 14.dp, Modifier.alpha(iconAlpha))
            Text(
                label,
                color = color,
                fontFamily = MonoFont,
                fontSize = 12.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = label.isNotEmpty()).padding(start = 8.dp),
            )
            trailing()
            YIcon(YIcons.ChevronRight, c.tx5, 14.dp, Modifier.padding(start = 6.dp).rotate(if (open) 90f else 0f))
            if (label.isEmpty()) Box(Modifier.weight(1f))
        }
        if (open) {
            SelectionContainer {
                Text(
                    body,
                    color = c.tx3,
                    fontFamily = if (prose) null else MonoFont,
                    fontSize = if (prose) 13.5.sp else 12.sp,
                    lineHeight = if (prose) 21.sp else 18.sp,
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(start = 22.dp, top = 2.dp, bottom = 6.dp)
                        .clip(FOLD_SHAPE)
                        .background(c.bgInset)
                        .border(1.dp, c.line, FOLD_SHAPE)
                        .padding(horizontal = 10.dp, vertical = 8.dp)
                        .testTag("fold-body"),
                )
            }
        }
    }
}

/** 受信箱の通知: 破線の控えめな枠 (色で警告しない)。 */
@Composable
private fun NoticeBox(item: TranscriptItem.Notice) {
    val c = Yhtye.colors
    val attrs = item.attrs.joinToString(" ") { it.joinToString("=") }
    Column(
        Modifier
            .fillMaxWidth()
            .dashedBorder(c.lineDashed, 8.dp)
            .padding(horizontal = 10.dp, vertical = 8.dp)
            .testTag("notice"),
    ) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), itemVerticalAlignment = Alignment.CenterVertically) {
            YIcon(YIcons.Bell, c.tx4, 12.dp)
            Text(item.inboxKind.wire, color = c.tx3, fontFamily = MonoFont, fontSize = 11.5.sp)
            if (attrs.isNotEmpty()) Text(attrs, color = c.tx4, fontFamily = MonoFont, fontSize = 11.5.sp)
        }
        if (item.body.isNotEmpty()) {
            Text(item.body, color = c.tx3, fontSize = 13.sp, lineHeight = 20.sp, modifier = Modifier.padding(top = 4.dp))
        }
    }
}

/** 会話の中のグループのカード (タスクは今の状態から読む)。 */
@Composable
private fun GroupCardView(item: TranscriptItem.GroupCard, state: State?) {
    val c = Yhtye.colors
    val tasks = state?.tasks.orEmpty().filter { it.group == item.groupId }
    Column(
        Modifier
            .fillMaxWidth()
            .clip(CARD_SHAPE)
            .border(1.dp, c.lineCard, CARD_SHAPE)
            .testTag("group-card"),
    ) {
        Row(
            Modifier.fillMaxWidth().background(c.bgInset).padding(horizontal = 10.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            YIcon(YIcons.Layers, c.tx3, 14.dp)
            Text(
                item.title,
                color = c.tx,
                fontSize = 13.5.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f).padding(horizontal = 7.dp),
            )
            Text(item.groupId, color = c.tx3, fontFamily = MonoFont, fontSize = 12.sp)
        }
        if (tasks.isEmpty()) {
            Text("タスクはまだありません", color = c.tx4, fontSize = 13.sp, modifier = Modifier.padding(10.dp))
        }
        tasks.forEach { t ->
            val tone = taskTone(t)
            Row(
                Modifier
                    .fillMaxWidth()
                    .drawBehind { drawLine(c.line, start = Offset.Zero, end = Offset(size.width, 0f)) }
                    .padding(horizontal = 10.dp, vertical = 7.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                YIcon(toneIcon(tone, t.kind), toneColor(tone), 14.dp)
                Text(t.id, color = c.tx4, fontFamily = MonoFont, fontSize = 11.5.sp, modifier = Modifier.padding(horizontal = 8.dp))
                Text(t.title, color = c.txBody, fontSize = 13.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

/** 状態の系統のアイコン (PC の statusIcons の `toneIcon`。調査タスクの実装中は虫眼鏡)。 */
fun toneIcon(tone: Tone, kind: TaskKind): ImageVector = when (tone) {
    Tone.Waiting -> YIcons.Hourglass
    Tone.Implementing -> if (kind == TaskKind.Investigate) YIcons.Search else YIcons.Code
    Tone.Reviewing -> YIcons.Eye
    Tone.Handling -> YIcons.Alert
    Tone.Done -> YIcons.Check
    Tone.Cancelled -> YIcons.Ban
}

@Composable
fun toneColor(tone: Tone): Color {
    val c = Yhtye.colors
    return when (tone) {
        Tone.Waiting -> c.tx3
        Tone.Implementing -> c.implBar
        Tone.Reviewing -> c.revBar
        Tone.Handling -> c.handBar
        Tone.Done -> c.doneBar
        Tone.Cancelled -> c.tx5
    }
}

/** 破線の角丸の枠。 */
fun Modifier.dashedBorder(color: Color, radius: Dp): Modifier = drawBehind {
    val w = 1.dp.toPx()
    drawRoundRect(
        color = color,
        topLeft = Offset(w / 2, w / 2),
        size = Size(size.width - w, size.height - w),
        cornerRadius = CornerRadius(radius.toPx()),
        style = Stroke(width = w, pathEffect = PathEffect.dashPathEffect(floatArrayOf(4.dp.toPx(), 3.dp.toPx()))),
    )
}

private val TIME_FORMAT: DateTimeFormatter = DateTimeFormatter.ofPattern("HH:mm")

fun formatTime(ms: Long): String = TIME_FORMAT.format(Instant.ofEpochMilli(ms).atZone(ZoneId.systemDefault()))
