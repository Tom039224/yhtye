package io.github.tom039224.yhtye.ui.settings

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.SettingsCommands
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons

/**
 * 設定 (積み重ねの画面。左パネルのレールの一番下から開く)。PC の設定画面と同じ節 — エージェント・ハーネス・秘密の環境変数 —
 * と、PC の「リモート接続」に当たるスマホ固有の「この端末」(接続先・常駐方式・通知・電池の最適化・ペアリング解除)。
 * 設定は PC のコアに保存され、変えるとすぐ PC の画面にも反映される。
 *
 * @param commands 設定の要求と変更の知らせ ([io.github.tom039224.yhtye.shared.sync.AppStore.settings])。
 * @param onBack ヘッダの戻る (スワイプ・戻るジェスチャは積み重ねの画面が扱う)。
 */
@Composable
fun SettingsScreen(
    app: AppState,
    commands: SettingsCommands,
    device: DevicePane,
    onBack: () -> Unit,
    onUnpair: () -> Unit,
) {
    val c = Yhtye.colors
    var tabKey by rememberSaveable { mutableStateOf(SettingsTab.Agents.key) }
    val tab = SettingsTab.parse(tabKey)
    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars).testTag("settings")) {
        Row(Modifier.fillMaxWidth().heightIn(min = 60.dp).padding(start = 4.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack, modifier = Modifier.semantics { contentDescription = "戻る" }.testTag("settings-back")) {
                YIcon(YIcons.Back, c.tx2, 20.dp)
            }
            Text("設定", color = c.tx, fontSize = 15.5.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 2.dp))
        }
        HorizontalDivider(color = c.line)
        SettingsTabRow(tab) { tabKey = it.key }
        HorizontalDivider(color = c.line)
        // キーボードが出たら (入力欄が隠れないよう) その分だけ縮める
        Column(Modifier.weight(1f).fillMaxWidth().windowInsetsPadding(WindowInsets.ime.union(WindowInsets.navigationBars))) {
            when (tab) {
                SettingsTab.Agents -> AgentSettingsTab(commands, app.project?.info)
                SettingsTab.Harnesses -> HarnessSettingsTab(commands)
                SettingsTab.Secrets -> SecretEnvTab(commands)
                SettingsTab.Device -> DeviceTab(app, device, onUnpair)
            }
        }
    }
}
