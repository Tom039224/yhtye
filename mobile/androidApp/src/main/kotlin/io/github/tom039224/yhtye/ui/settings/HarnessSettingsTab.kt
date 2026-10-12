package io.github.tom039224.yhtye.ui.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.snapshots.SnapshotStateMap
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.HarnessDetection
import io.github.tom039224.yhtye.shared.sync.EMPTY_PATH_MESSAGE
import io.github.tom039224.yhtye.shared.sync.HarnessSettingsController
import io.github.tom039224.yhtye.shared.sync.PATH_SOURCE_LABELS
import io.github.tom039224.yhtye.shared.sync.SettingsCommands
import io.github.tom039224.yhtye.shared.sync.draftsOf
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.launch

const val HARNESS_NOTE =
    "エージェントを動かすハーネスの検出状態です。実行ファイルは PATH と既知の場所 (~/.local/bin, ~/.cargo/bin, ~/.bun/bin, " +
        "/usr/local/bin) から探します。見つからないときは主実行ファイルの絶対パスを手動で指定できます (手動パスは自動検出より優先)。" +
        "インストール済みのハーネスがエージェント設定で選べます。"

/**
 * 「ハーネス」の節 (PC の HarnessSettingsSection): 各ハーネスが入っているか、見つかった場所、主実行ファイルの手動パス
 * (自動検出より優先)。開くとコアが探し直す。他の端末の変更は読み直し、パスを入力中ならバナー。
 */
@Composable
fun HarnessSettingsTab(commands: SettingsCommands) {
    val scope = rememberCoroutineScope()
    val controller = remember(commands) { HarnessSettingsController(commands, scope) }
    LaunchedEffect(controller) { controller.reload() }
    val state by controller.state.collectAsState()
    val stale by controller.stale.collectAsState()
    val list = state.harnesses
    // 入力中の手動パス (読み直すたびに保存した値へ戻す)
    val drafts: SnapshotStateMap<String, String> = remember(controller, state.epoch) {
        mutableStateMapOf<String, String>().apply { putAll(draftsOf(list.orEmpty())) }
    }
    val unsaved = list?.any { (drafts[it.id] ?: "") != (it.overridePath ?: "") } ?: false
    SideEffect { controller.unsaved = unsaved }
    SettingsPane("harness-section", stale, controller::reloadNow) {
        Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            NoteText(HARNESS_NOTE, Modifier.weight(1f))
        }
        OutlineButton(
            "再検出",
            onClick = controller::detect,
            icon = YIcons.Refresh,
            enabled = !state.busy && !(list == null && state.error == null),
            spin = state.detecting || (list == null && state.error == null),
            modifier = Modifier.semantics { contentDescription = "ハーネスを再検出" },
            tag = "detect-harnesses",
        )
        state.error?.let { WarningLine(it, tag = "harness-error") }
        if (list == null) {
            if (state.error == null) StatusLine("検出中…")
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                list.forEach { h ->
                    HarnessCard(
                        harness = h,
                        draft = drafts[h.id] ?: "",
                        error = state.rowErrors[h.id],
                        busy = state.busy,
                        onDraft = { drafts[h.id] = it },
                        onFocus = { controller.editing.set("path-${h.id}", it) },
                        onSubmit = {
                            val path = (drafts[h.id] ?: "").trim()
                            if (path.isEmpty()) {
                                controller.rowError(h.id, EMPTY_PATH_MESSAGE)
                            } else {
                                scope.launch { if (controller.setPath(h.id, path)) drafts[h.id] = path }
                            }
                        },
                        onReset = {
                            scope.launch { if (controller.setPath(h.id, null)) drafts[h.id] = "" }
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun HarnessCard(
    harness: HarnessDetection,
    draft: String,
    error: String?,
    busy: Boolean,
    onDraft: (String) -> Unit,
    onFocus: (Boolean) -> Unit,
    onSubmit: () -> Unit,
    onReset: () -> Unit,
) {
    val c = Yhtye.colors
    val main = harness.requirements.firstOrNull()?.command ?: harness.id
    SettingsCard(Modifier.semantics { contentDescription = harness.label }.testTag("harness-${harness.id}")) {
        Column(Modifier.padding(horizontal = 12.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(harness.label, color = c.tx, fontSize = 14.sp)
                Text(harness.id, color = c.tx4, fontFamily = MonoFont, fontSize = 11.5.sp, modifier = Modifier.weight(1f))
                if (harness.installed) {
                    SmallChip("インストール済み", c.doneBg, c.doneFg, icon = YIcons.Check)
                } else {
                    SmallChip("未インストール", c.handBg, c.handFg, icon = YIcons.Alert)
                }
            }
            harness.requirements.forEachIndexed { i, r ->
                Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text(r.command, color = c.tx2, fontFamily = MonoFont, fontSize = 12.sp)
                        if (i == 0) {
                            if (r.found != null) SmallChip(PATH_SOURCE_LABELS[harness.pathSource] ?: "—", c.bgChip, c.tx2)
                        } else {
                            Text("自動検出のみ", color = c.tx4, fontSize = 11.5.sp)
                        }
                    }
                    if (r.found != null) {
                        Text(r.found!!, color = c.tx3, fontFamily = MonoFont, fontSize = 11.5.sp, lineHeight = 17.sp)
                    } else {
                        Text("見つかりません", color = c.handFg, fontSize = 12.sp)
                    }
                }
            }
            harness.overrideError?.let {
                WarningLine("手動パス ${harness.overridePath} を使えません: $it", tag = "override-error-${harness.id}")
            }
            error?.let { WarningLine(it, tag = "path-error-${harness.id}") }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                SettingsField(
                    value = draft,
                    onChange = onDraft,
                    placeholder = "$main の絶対パス (例 /opt/bin/$main)",
                    label = "${harness.label} の $main のパス",
                    tag = "path-${harness.id}",
                    modifier = Modifier.weight(1f),
                    onDone = onSubmit,
                    onFocus = onFocus,
                )
                IconAction(YIcons.Check, "${harness.label} の手動パスを保存", onSubmit, enabled = !busy, tag = "path-save-${harness.id}")
                IconAction(
                    YIcons.X,
                    "${harness.label} を自動検出に戻す",
                    onReset,
                    enabled = !busy && harness.overridePath != null,
                    tag = "path-reset-${harness.id}",
                )
            }
        }
    }
}
