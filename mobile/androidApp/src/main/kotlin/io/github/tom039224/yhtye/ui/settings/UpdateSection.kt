package io.github.tom039224.yhtye.ui.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.update.UpdateState
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.panels.formatDateTime
import kotlinx.coroutines.flow.StateFlow

/** 設定の「アプリの更新」に要るもの: 確認の状態と、その操作 (今すぐ確認・自動の確認の切り替え)。 */
class UpdatePane(
    val state: StateFlow<UpdateState>,
    val onCheckNow: () -> Unit,
    val onEnabled: (Boolean) -> Unit,
)

/**
 * アプリの更新 (T-84)。新しい版があれば知らせ、「ダウンロード」でそのリリースのページをブラウザで開く
 * (APK のダウンロードとインストールはブラウザと OS がする。このアプリは GitHub から何も保存しない)。
 * 確認に失敗した理由はここにだけ出す。
 */
@Composable
fun UpdateSection(updates: UpdatePane, system: SystemAccess) {
    val c = Yhtye.colors
    val state by updates.state.collectAsStateWithLifecycle()
    Section(stringResource(R.string.update_title), YIcons.Download, "device-update") {
        NoteText(stringResource(R.string.update_intro), Modifier.padding(horizontal = 12.dp, vertical = 8.dp))
        CheckRow(
            stringResource(R.string.update_enabled),
            stringResource(R.string.update_enabled_desc),
            state.enabled,
            updates.onEnabled,
        )
        state.available?.let { update ->
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(
                    stringResource(R.string.update_available, update.version.toString()),
                    color = c.tx,
                    fontSize = 14.sp,
                    modifier = Modifier.weight(1f).testTag("update-available"),
                )
                OutlineButton(
                    stringResource(R.string.update_download),
                    onClick = { system.openUrl(update.pageUrl) },
                    icon = YIcons.Download,
                    tag = "update-download",
                )
            }
        }
        Column(Modifier.padding(horizontal = 12.dp, vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            val checkedAt = state.checkedAt
            StatusLine(
                when {
                    state.checking -> stringResource(R.string.update_checking)
                    checkedAt == null -> stringResource(R.string.update_never_checked)
                    else -> stringResource(R.string.update_last_checked, formatDateTime(checkedAt))
                },
                tag = "update-checked",
            )
            if (state.enabled && !state.checking && checkedAt != null && state.available == null && state.failure == null) {
                StatusLine(stringResource(R.string.update_up_to_date), tag = "update-up-to-date")
            }
            state.failure?.let { StatusLine(stringResource(R.string.update_failed, it), error = true, tag = "update-failure") }
        }
        OutlineButton(
            stringResource(R.string.update_check_now),
            onClick = updates.onCheckNow,
            modifier = Modifier.padding(start = 12.dp, end = 12.dp, bottom = 12.dp),
            icon = YIcons.Refresh,
            enabled = state.enabled && !state.checking,
            spin = state.checking,
            tag = "update-check-now",
        )
    }
}

/** チェックボックスの行 (押せる範囲は行全体。説明を下に付ける)。 */
@Composable
private fun CheckRow(title: String, description: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    val c = Yhtye.colors
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clickable(role = Role.Checkbox) { onChange(!checked) }
            .padding(horizontal = 4.dp, vertical = 6.dp)
            .testTag("update-enabled"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(
            checked = checked,
            onCheckedChange = null,
            colors = CheckboxDefaults.colors(checkedColor = c.accent, uncheckedColor = c.tx4, checkmarkColor = c.onAccent),
            modifier = Modifier.padding(horizontal = 8.dp),
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, color = c.tx, fontSize = 14.sp)
            Text(description, color = c.tx3, fontSize = 12.5.sp, lineHeight = 19.sp)
        }
    }
}
