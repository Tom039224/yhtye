package io.github.tom039224.yhtye.ui.conversation

import android.net.Uri
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.spring
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.content.ReceiveContentListener
import androidx.compose.foundation.content.consume
import androidx.compose.foundation.content.contentReceiver
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
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
import io.github.tom039224.yhtye.shared.sync.PendingAttachment
import io.github.tom039224.yhtye.shared.sync.canSendMessage
import io.github.tom039224.yhtye.shared.sync.isSettled
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch

private val BOX_SHAPE = RoundedCornerShape(24.dp)
private val CHIP_SHAPE = RoundedCornerShape(15.dp)
private val INPUT_MAX_HEIGHT = 128.dp
private const val CHIP_POP_FROM = 0.4f

/**
 * コンポーザ (PC の Composer、モックの `.composer`): 複数行の本文、左下に添付の +、右下に送信 (ターン中は中止も)、
 * 入力欄の上に添付のタイル、下にメンションのチップと状態の一言。画像はクリップボード・キーボードから貼り付けても
 * 添付になる。添付がすべてステージできていれば本文が空でも送れる。ターン中も送れる (受信箱で待ち、ターンが
 * 終わったら届く)。送れたら本文を空にする (楽観的に会話には足さない。発言はコアのイベントで出る)。
 * 呼ぶ側がチャットごとに作り直す (下書きを [initialText] で渡す)。
 *
 * @param attachments このチャットの入力中の添付 (追加した順)。
 * @param loadAttachment ステージした画像のサムネイルを取る。
 * @param onAddFiles 選んだ・貼り付けたファイル (呼ぶ側がステージする)。null なら + を出さない。
 * @param onRemoveAttachment タイルの × (入力中の添付の localId)。
 */
@OptIn(ExperimentalFoundationApi::class)
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
    attachments: List<PendingAttachment> = emptyList(),
    loadAttachment: LoadAttachment? = null,
    onAddFiles: ((List<Uri>) -> Unit)? = null,
    onRemoveAttachment: (Long) -> Unit = {},
) {
    val c = Yhtye.colors
    val scope = rememberCoroutineScope()
    val field = rememberTextFieldState(initialText)
    val latestOnTextChange by rememberUpdatedState(onTextChange)
    LaunchedEffect(field) {
        snapshotFlow { field.text.toString() }.drop(1).collect { latestOnTextChange(it) }
    }
    var focused by remember { mutableStateOf(false) }
    val disabled = disabledReason != null
    val text = field.text.toString()
    val canSend = !disabled && !sending && canSendMessage(text, attachments)
    val canAttach = !disabled && onAddFiles != null
    val send: () -> Unit = {
        if (canSend) {
            scope.launch { if (onSend(text)) field.clearText() }
        }
    }
    val uploading = attachments.any { it.upload is PendingAttachment.Upload.Uploading }
    val attachHint = when {
        attachments.isSettled() -> null
        uploading -> "添付中… · 終わると送信できます"
        else -> "添付できなかったファイルを外すと送信できます"
    }
    val hint = disabledReason ?: when {
        starting -> "起動中…"
        attachHint != null -> attachHint
        turnRunning -> "作業中 · 送信分はターン終了後に届きます"
        else -> null
    }
    val hintIcon = when {
        disabledReason != null -> HintIcon.Error
        starting -> HintIcon.Busy
        attachHint != null -> if (uploading) HintIcon.Busy else HintIcon.Error
        else -> HintIcon.Waiting
    }
    // 貼り付け・キーボードの画像 (`content:` の Uri)。文字の項目はそのまま入力欄に入る
    val paste = remember(canAttach, onAddFiles) {
        ReceiveContentListener { content ->
            if (!canAttach || onAddFiles == null) return@ReceiveContentListener content
            val uris = mutableListOf<Uri>()
            val rest = content.consume { item ->
                isPastedFile(item).also { file -> if (file) uris += item.uri }
            }
            if (uris.isNotEmpty()) onAddFiles(uris)
            rest
        }
    }

    Column(modifier.fillMaxWidth().padding(start = 10.dp, end = 10.dp, top = 4.dp, bottom = 10.dp)) {
        Column(
            Modifier
                .fillMaxWidth()
                .clip(BOX_SHAPE)
                .background(c.bgInset)
                .border(1.dp, if (focused) c.accent.copy(alpha = 0.45f) else c.lineCard, BOX_SHAPE)
                .padding(4.dp),
        ) {
            if (attachments.isNotEmpty()) {
                PendingAttachments(
                    attachments,
                    loadAttachment,
                    onRemove = onRemoveAttachment,
                    modifier = Modifier.padding(start = 6.dp, end = 6.dp, top = 6.dp, bottom = 2.dp),
                )
            }
            Row(
                Modifier.fillMaxWidth().padding(start = if (onAddFiles != null) 0.dp else 12.dp),
                verticalAlignment = Alignment.Bottom,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                if (onAddFiles != null) AttachButton(enabled = canAttach, onAddFiles = onAddFiles)
                BasicTextField(
                    state = field,
                    enabled = !disabled,
                    textStyle = TextStyle(color = c.tx, fontSize = 15.5.sp, lineHeight = 22.sp),
                    cursorBrush = SolidColor(c.accent),
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = 40.dp, max = INPUT_MAX_HEIGHT)
                        .padding(vertical = 9.dp)
                        .contentReceiver(paste)
                        .onFocusChanged { focused = it.isFocused }
                        .semantics { contentDescription = "オーケストレータへのメッセージ" }
                        .testTag("composer-input"),
                    decorator = { inner ->
                        Box {
                            if (field.text.isEmpty()) Text("オーケストレータに指示する…", color = c.tx4, fontSize = 15.5.sp, lineHeight = 22.sp)
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
        }
        if (mentions.isNotEmpty() || hint != null) {
            FlowRow(
                Modifier.fillMaxWidth().padding(start = 4.dp, end = 4.dp, top = 6.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
                itemVerticalAlignment = Alignment.CenterVertically,
            ) {
                mentions.forEach { m -> MentionChip(m) { onRemoveMention(m.task) } }
                if (hint != null) Hint(hint, hintIcon)
            }
        }
    }
}

/** 状態の一言のアイコン (失敗・待ち (回る)・ターン中)。 */
private enum class HintIcon { Error, Busy, Waiting }

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
private fun Hint(text: String, icon: HintIcon) {
    val c = Yhtye.colors
    val error = icon == HintIcon.Error
    Row(
        Modifier.semantics { liveRegion = LiveRegionMode.Polite }.testTag("composer-hint"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        YIcon(
            when (icon) {
                HintIcon.Error -> YIcons.Alert
                HintIcon.Busy -> YIcons.Loader
                HintIcon.Waiting -> YIcons.Clock
            },
            if (error) c.handBar else c.tx3,
            12.dp,
            spin = icon == HintIcon.Busy,
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
