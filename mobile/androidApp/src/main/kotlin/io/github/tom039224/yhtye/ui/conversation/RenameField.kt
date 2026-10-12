package io.github.tom039224.yhtye.ui.conversation

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.client.RpcException
import io.github.tom039224.yhtye.shared.sync.MAX_CHAT_TITLE_CHARS
import io.github.tom039224.yhtye.shared.sync.describe
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.launch

private val FIELD_SHAPE = RoundedCornerShape(8.dp)

/**
 * チャットの名前を上部バーのその場で変える (PC の RenameForm: 行のタイトルが入力欄になる)。完了 (IME) か ✓ で保存、
 * ✕ か戻るで取り消す。空・長すぎるなどコアが断った理由は入力欄の下に出す。保存はコアの応答で反映する。
 */
@Composable
fun RenameField(current: String, onDone: () -> Unit, rename: suspend (String) -> Unit) {
    val c = Yhtye.colors
    val scope = rememberCoroutineScope()
    var value by remember { mutableStateOf(TextFieldValue(current, TextRange(0, current.length))) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    BackHandler(enabled = !busy, onBack = onDone)
    val save: () -> Unit = save@{
        if (busy) return@save
        val title = value.text.trim()
        if (title == current) return@save onDone()
        if (title.isEmpty()) {
            error = "名前を入力してください"
            return@save
        }
        busy = true
        error = null
        scope.launch {
            try {
                rename(title)
                onDone()
            } catch (e: RpcException) {
                error = describe(e)
                busy = false
            }
        }
    }
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            BasicTextField(
                value = value,
                onValueChange = { if (it.text.length <= MAX_CHAT_TITLE_CHARS) value = it },
                enabled = !busy,
                singleLine = true,
                textStyle = TextStyle(color = c.tx, fontSize = 16.sp, fontWeight = FontWeight.SemiBold),
                cursorBrush = SolidColor(c.accent),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { save() }),
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, if (error != null) c.handBar else c.accent.copy(alpha = 0.45f), FIELD_SHAPE)
                    .padding(horizontal = 8.dp, vertical = 6.dp)
                    .focusRequester(focus)
                    .semantics { contentDescription = "チャットの名前" }
                    .testTag("rename-input"),
                decorationBox = { inner ->
                    Box {
                        if (value.text.isEmpty()) Text(NEW_CHAT_TITLE, color = c.tx4, fontSize = 16.sp)
                        inner()
                    }
                },
            )
            error?.let {
                Text(
                    it,
                    color = c.handFg,
                    fontSize = 11.5.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(start = 2.dp, top = 2.dp).semantics { liveRegion = LiveRegionMode.Polite }.testTag("rename-error"),
                )
            }
        }
        IconButton(onClick = save, enabled = !busy, modifier = Modifier.semantics { contentDescription = "保存" }.testTag("rename-confirm")) {
            YIcon(YIcons.Check, c.doneBar, 20.dp)
        }
        IconButton(onClick = onDone, enabled = !busy, modifier = Modifier.semantics { contentDescription = "キャンセル" }) {
            YIcon(YIcons.X, c.tx3, 20.dp)
        }
    }
}
