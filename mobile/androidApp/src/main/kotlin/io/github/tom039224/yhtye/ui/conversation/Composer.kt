package io.github.tom039224.yhtye.ui.conversation

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.launch

private val BOX_SHAPE = RoundedCornerShape(24.dp)
private val CHIP_SHAPE = RoundedCornerShape(15.dp)
private val INPUT_MAX_HEIGHT = 128.dp
private const val CHIP_POP_FROM = 0.4f

/**
 * コンポーザ (PC の Composer、モックの `.composer`): 複数行の本文、右下に送信 (ターン中は中止も)、下にメンションの
 * チップと状態の一言。ターン中も送れる (受信箱で待ち、ターンが終わったら届く)。送れたら本文を空にする
 * (楽観的に会話には足さない。発言はコアのイベントで出る)。呼ぶ側がチャットごとに作り直す (下書きを [initialText] で渡す)。
 */
@Composable
fun Composer(
    initialText: String,
    onTextChange: (String) -> Unit,
    disabledReason: String?,
    sending: Boolean,
    turnRunning: Boolean,
    starting: Boolean,
    mentions: List<Mention>,
    onRemoveMention: (String) -> Unit,
    onSend: suspend (String) -> Boolean,
    onCancel: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val c = Yhtye.colors
    val scope = rememberCoroutineScope()
    var text by rememberSaveable { mutableStateOf(initialText) }
    var focused by remember { mutableStateOf(false) }
    val disabled = disabledReason != null
    val canSend = !disabled && !sending && text.isNotBlank()
    val edit = { next: String ->
        text = next
        onTextChange(next)
    }
    val send: () -> Unit = {
        if (canSend) {
            val sent = text
            scope.launch { if (onSend(sent)) edit("") }
        }
    }
    val hint = disabledReason ?: when {
        starting -> "起動中…"
        turnRunning -> "作業中 · 送信分はターン終了後に届きます"
        else -> null
    }

    androidx.compose.foundation.layout.Column(modifier.fillMaxWidth().padding(start = 10.dp, end = 10.dp, top = 4.dp, bottom = 10.dp)) {
        Row(
            Modifier
                .fillMaxWidth()
                .clip(BOX_SHAPE)
                .background(c.bgInset)
                .border(1.dp, if (focused) c.accent.copy(alpha = 0.45f) else c.lineCard, BOX_SHAPE)
                .padding(start = 16.dp, top = 4.dp, end = 4.dp, bottom = 4.dp),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            BasicTextField(
                value = text,
                onValueChange = edit,
                enabled = !disabled,
                textStyle = TextStyle(color = c.tx, fontSize = 15.5.sp, lineHeight = 22.sp),
                cursorBrush = SolidColor(c.accent),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 40.dp, max = INPUT_MAX_HEIGHT)
                    .padding(vertical = 9.dp)
                    .onFocusChanged { focused = it.isFocused }
                    .semantics { contentDescription = "オーケストレータへのメッセージ" }
                    .testTag("composer-input"),
                decorationBox = { inner ->
                    Box {
                        if (text.isEmpty()) Text("オーケストレータに指示する…", color = c.tx4, fontSize = 15.5.sp, lineHeight = 22.sp)
                        inner()
                    }
                },
            )
            if (turnRunning) {
                RoundButton(
                    label = "ターンを中止",
                    enabled = !disabled,
                    onClick = onCancel,
                    testTag = "composer-stop",
                    background = null,
                ) { YIcon(YIcons.Stop, c.handFg, 13.dp) }
            }
            RoundButton(
                label = if (sending) "送信中…" else "送信",
                enabled = canSend,
                onClick = send,
                testTag = "composer-send",
                background = if (canSend) c.accent else c.bgChip,
            ) { YIcon(YIcons.Send, if (canSend) c.onAccent else c.tx5, 20.dp) }
        }
        if (mentions.isNotEmpty() || hint != null) {
            FlowRow(
                Modifier.fillMaxWidth().padding(start = 4.dp, end = 4.dp, top = 6.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
                itemVerticalAlignment = Alignment.CenterVertically,
            ) {
                mentions.forEach { m -> MentionChip(m) { onRemoveMention(m.task) } }
                if (hint != null) Hint(hint, error = disabledReason != null, starting = starting)
            }
        }
    }
}

@Composable
private fun RoundButton(
    label: String,
    enabled: Boolean,
    onClick: () -> Unit,
    testTag: String,
    background: androidx.compose.ui.graphics.Color?,
    content: @Composable () -> Unit,
) {
    val c = Yhtye.colors
    Box(
        Modifier
            .size(40.dp)
            .clip(CircleShape)
            .then(if (background != null) Modifier.background(background) else Modifier.border(1.dp, c.lineCard, CircleShape))
            .clickable(enabled = enabled, role = Role.Button, onClickLabel = label, onClick = onClick)
            .semantics { contentDescription = label }
            .testTag(testTag),
        contentAlignment = Alignment.Center,
    ) { content() }
}

/** 積んだメンション (タップで外す)。積んだときに弾む (モックの `chipin`)。 */
@Composable
private fun MentionChip(m: Mention, onRemove: () -> Unit) {
    val c = Yhtye.colors
    val pop = remember { Animatable(CHIP_POP_FROM) }
    LaunchedEffect(Unit) { pop.animateTo(1f, spring(dampingRatio = 0.45f, stiffness = 600f)) }
    Row(
        Modifier
            .scale(pop.value)
            .height(30.dp)
            .widthIn(max = 240.dp)
            .clip(CHIP_SHAPE)
            .background(c.accentSoft)
            .clickable(role = Role.Button, onClick = onRemove)
            .semantics { contentDescription = "${m.task} の引用を外す" }
            .padding(start = 8.dp, end = 6.dp)
            .testTag("mention-${m.task}"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        YIcon(YIcons.At, c.accent, 14.dp)
        Text(
            "${m.task} ${m.title}",
            color = c.accent,
            fontFamily = MonoFont,
            fontSize = 12.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f, fill = false).padding(horizontal = 4.dp),
        )
        YIcon(YIcons.X, c.accent, 12.dp)
    }
}

@Composable
private fun Hint(text: String, error: Boolean, starting: Boolean) {
    val c = Yhtye.colors
    Row(
        Modifier.semantics { liveRegion = LiveRegionMode.Polite }.testTag("composer-hint"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        YIcon(
            when {
                error -> YIcons.Alert
                starting -> YIcons.Loader
                else -> YIcons.Clock
            },
            if (error) c.handBar else c.tx3,
            12.dp,
            spin = !error && starting,
        )
        Text(
            text,
            color = if (error) c.handFg else c.tx3,
            fontFamily = MonoFont,
            fontSize = 11.5.sp,
            lineHeight = 15.sp,
            modifier = Modifier.padding(start = 5.dp),
        )
    }
}
