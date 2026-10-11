package io.github.tom039224.yhtye.ui.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.sync.RegisterResult
import io.github.tom039224.yhtye.shared.sync.SecretEnvController
import io.github.tom039224.yhtye.shared.sync.SettingsCommands
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.launch

const val SECRET_NOTE =
    "エージェントに渡す秘密の環境変数 (API キーなど) を登録します。値は OS のキーリング (Secret Service など) に保存され、" +
        "Yhtye は名前だけを覚えます。値は保存後に表示も取得もできません (上書きか削除だけ)。登録した変数は、これから起動する" +
        "すべてのエージェントの環境に入ります。"

/**
 * 「秘密の環境変数」の節 (PC の SecretEnvSection): 登録されている名前の一覧と、追加・上書き・削除。値は送るだけで、
 * 画面にも状態にも残さない (失敗しても捨てる)。他の端末の変更は読み直し、入力中ならバナー。
 */
@Composable
fun SecretEnvTab(commands: SettingsCommands) {
    val scope = rememberCoroutineScope()
    val controller = remember(commands) { SecretEnvController(commands, scope) }
    LaunchedEffect(controller) { controller.reload() }
    val state by controller.state.collectAsState()
    val stale by controller.stale.collectAsState()
    var name by remember { mutableStateOf("") }
    var value by remember { mutableStateOf("") }
    var confirming by remember { mutableStateOf<String?>(null) }
    val valueFocus = remember { FocusRequester() }
    val submit: () -> Unit = {
        scope.launch {
            when (controller.register(name, value)) {
                RegisterResult.Saved -> {
                    name = ""
                    value = ""
                }
                // 失敗しても値は捨てる (秘密を画面に残さない)
                RegisterResult.Failed -> value = ""
                RegisterResult.Invalid -> Unit
            }
        }
    }
    SettingsPane("secret-section", stale, controller::reloadNow) {
        // 入力欄の状態を読むのはこの中なので、ここで編集中かを教える (外側は入力で再構成されない)
        SideEffect { controller.editing.set("form", name.isNotEmpty() || value.isNotEmpty() || confirming != null) }
        NoteText(SECRET_NOTE)
        state.error?.let { WarningLine(it, tag = "secret-error") }
        Column(verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.semantics { contentDescription = "秘密の環境変数を登録" }) {
            SettingsField(name, { name = it }, "NAME (例 OPENROUTER_API_KEY_CODEX)", "変数名", "secret-name")
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SettingsField(
                    value = value,
                    onChange = { value = it },
                    placeholder = "値",
                    label = "値",
                    tag = "secret-value",
                    modifier = Modifier.weight(1f).focusRequester(valueFocus),
                    password = true,
                    onDone = submit,
                )
                OutlineButton("登録", onClick = submit, icon = YIcons.Plus, enabled = !state.busy, tag = "secret-register")
            }
        }
        val names = state.names
        when {
            names == null -> if (state.error == null) StatusLine("読み込み中…")
            names.isEmpty() -> StatusLine("登録されている変数はありません。", tag = "secret-empty")
            else -> Column(verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.semantics { contentDescription = "登録済みの環境変数" }) {
                names.forEach { n ->
                    SecretRow(
                        name = n,
                        confirming = confirming == n,
                        busy = state.busy,
                        onOverwrite = {
                            name = n
                            value = ""
                            controller.clearError()
                            valueFocus.requestFocus()
                        },
                        onAskDelete = { confirming = n },
                        onCancel = { confirming = null },
                        onDelete = {
                            confirming = null
                            scope.launch { controller.remove(n) }
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun SecretRow(
    name: String,
    confirming: Boolean,
    busy: Boolean,
    onOverwrite: () -> Unit,
    onAskDelete: () -> Unit,
    onCancel: () -> Unit,
    onDelete: () -> Unit,
) {
    val c = Yhtye.colors
    SettingsCard(Modifier.testTag("secret-row-$name")) {
        Row(
            Modifier.fillMaxWidth().heightIn(min = 52.dp).padding(start = 12.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            if (confirming) {
                Text("$name を削除しますか?", color = c.tx2, fontSize = 13.sp, modifier = Modifier.weight(1f))
                IconAction(YIcons.Trash, "削除する", onDelete, enabled = !busy, tint = c.handFg, tag = "secret-delete-confirm")
                IconAction(YIcons.X, "やめる", onCancel, tag = "secret-delete-cancel")
            } else {
                Text(name, color = c.tx, fontFamily = MonoFont, fontSize = 12.5.sp, modifier = Modifier.weight(1f))
                SmallChip("登録済み", c.doneBg, c.doneFg, icon = YIcons.Check)
                IconAction(YIcons.Edit, "$name を上書き", onOverwrite, tag = "secret-overwrite-$name")
                IconAction(YIcons.Trash, "$name を削除", onAskDelete, tag = "secret-delete-$name")
            }
        }
    }
}
