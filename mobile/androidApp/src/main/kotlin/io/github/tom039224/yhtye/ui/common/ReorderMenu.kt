package io.github.tom039224.yhtye.ui.common

import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.sync.step
import androidx.compose.ui.unit.dp

// 長押しで開く「上へ移動 / 下へ移動」(PC の右クリックメニューと Alt + ↑↓ に当たる。決定事項: スマホはドラッグでなくメニュー)。
// [keys] は画面に出ている行のキーの順、[key] は長押しした行。端では片方が選べない。選ぶと [onMove] が並べ替えた
// 全体の順を受け取る (コアに送って、他のクライアントにも反映される)。

/** @param tag テスト用の印: 項目は `$tag-up` / `$tag-down`。 */
@Composable
fun ReorderMenu(expanded: Boolean, onDismiss: () -> Unit, keys: List<String>, key: String, tag: String, onMove: (List<String>) -> Unit) {
    val c = Yhtye.colors
    val up = step(keys, key, -1)
    val down = step(keys, key, 1)
    DropdownMenu(expanded = expanded, onDismissRequest = onDismiss) {
        DropdownMenuItem(
            text = { Text("上へ移動") },
            leadingIcon = { YIcon(YIcons.ArrowUp, c.tx3, 18.dp) },
            enabled = up != null,
            onClick = {
                onDismiss()
                up?.let(onMove)
            },
            modifier = Modifier.testTag("$tag-up"),
        )
        DropdownMenuItem(
            text = { Text("下へ移動") },
            leadingIcon = { YIcon(YIcons.ArrowDown, c.tx3, 18.dp) },
            enabled = down != null,
            onClick = {
                onDismiss()
                down?.let(onMove)
            },
            modifier = Modifier.testTag("$tag-down"),
        )
    }
}
