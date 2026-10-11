package io.github.tom039224.yhtye.ui.settings

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons

// 仮の設定画面 (積み重ねの画面。左パネルのレールの一番下から開く)。T-71 が本番 (PC の設定の項目のうちエージェント /
// ハーネス / 秘密の環境変数、通知・常駐の設定) に差し替える。今はスマホ固有のペアリング解除 (決定事項) だけを置く。

private val CARD_SHAPE = RoundedCornerShape(12.dp)

/** @param onBack ヘッダの戻る (スワイプ・戻るジェスチャは積み重ねの画面が扱う)。 */
@Composable
fun SettingsScreen(app: AppState, onBack: () -> Unit, onUnpair: () -> Unit) {
    val c = Yhtye.colors
    var confirmUnpair by rememberSaveable { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars).testTag("settings")) {
        Row(Modifier.fillMaxWidth().heightIn(min = 60.dp).padding(start = 4.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack, modifier = Modifier.semantics { contentDescription = "戻る" }.testTag("settings-back")) {
                YIcon(YIcons.Back, c.tx2, 20.dp)
            }
            Text("設定", color = c.tx, fontSize = 15.5.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 2.dp))
        }
        HorizontalDivider(color = c.line)
        Column(
            Modifier.fillMaxWidth().windowInsetsPadding(WindowInsets.navigationBars).padding(horizontal = 14.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Column(Modifier.fillMaxWidth().clip(CARD_SHAPE).border(1.dp, c.lineCard, CARD_SHAPE)) {
                Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                    YIcon(YIcons.Unplug, c.tx3, 16.dp)
                    Text("接続先", color = c.tx, fontSize = 14.sp, modifier = Modifier.weight(1f).padding(start = 8.dp))
                    Text(app.host.name ?: "—", color = c.tx3, fontFamily = MonoFont, fontSize = 12.5.sp)
                }
                HorizontalDivider(color = c.line)
                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp)
                        .clickable(role = Role.Button) { confirmUnpair = true }
                        .padding(horizontal = 12.dp)
                        .testTag("unpair"),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(stringResource(R.string.home_unpair), color = c.handFg, fontSize = 14.sp)
                }
            }
        }
    }
    if (confirmUnpair) {
        AlertDialog(
            onDismissRequest = { confirmUnpair = false },
            title = { Text(stringResource(R.string.unpair_title)) },
            text = { Text(stringResource(R.string.unpair_message)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmUnpair = false
                        onUnpair()
                    },
                    modifier = Modifier.testTag("unpairConfirm"),
                ) { Text(stringResource(R.string.home_unpair)) }
            },
            dismissButton = { TextButton(onClick = { confirmUnpair = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}
