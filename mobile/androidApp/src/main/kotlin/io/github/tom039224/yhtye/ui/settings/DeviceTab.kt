package io.github.tom039224.yhtye.ui.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.BackgroundMode
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.flow.StateFlow

/** 設定の「この端末」に要るもの: 接続先、常駐方式 (保存先)、OS の許可の操作。 */
class DevicePane(
    val credentials: Credentials?,
    val mode: StateFlow<BackgroundMode>,
    val onMode: (BackgroundMode) -> Unit,
    val system: SystemAccess,
)

/** ペアリング情報も OS の操作も無い設定 (テストの既定)。 */
val UnavailableDevicePane = DevicePane(null, kotlinx.coroutines.flow.MutableStateFlow(BackgroundMode.DEFAULT), {}, SystemAccess.None)

/**
 * 「この端末」の節 (PC の「リモート接続」に当たるスマホ固有の欄。android-client.md の決定): 接続先の PC と状態、
 * 常駐方式 (常に / 作業中だけ)、通知の許可、電池の最適化、ペアリング解除。
 */
@Composable
fun DeviceTab(app: AppState, device: DevicePane, onUnpair: () -> Unit) {
    val c = Yhtye.colors
    val mode by device.mode.collectAsState()
    var notifications by remember { mutableStateOf(device.system.notificationState()) }
    var unrestricted by remember { mutableStateOf(device.system.isIgnoringBatteryOptimizations()) }
    var confirmUnpair by rememberSaveable { mutableStateOf(false) }
    // 許可の画面や OS の設定から戻ったら読み直す
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        notifications = device.system.notificationState()
        unrestricted = device.system.isIgnoringBatteryOptimizations()
    }
    SettingsPane("device-section") {
        Section(stringResource(R.string.device_connection_title), YIcons.Unplug, "device-connection") {
            InfoRow(stringResource(R.string.device_pc), app.host.name ?: "—", "device-pc")
            device.credentials?.let {
                InfoRow(stringResource(R.string.device_address), "${it.hosts.first()}:${it.port}", "device-address")
                InfoRow(stringResource(R.string.device_name), it.deviceName, "device-name")
            }
            InfoRow(stringResource(R.string.device_state), connectionText(app.connection), "device-state")
        }
        Section(stringResource(R.string.device_background_title), YIcons.Activity, "device-background") {
            NoteText(stringResource(R.string.device_background_intro), Modifier.padding(horizontal = 12.dp, vertical = 8.dp))
            ModeRow(BackgroundMode.Always, stringResource(R.string.device_background_always), stringResource(R.string.device_background_always_desc), mode, device.onMode)
            HorizontalDivider(color = c.line)
            ModeRow(BackgroundMode.WorkOnly, stringResource(R.string.device_background_work_only), stringResource(R.string.device_background_work_only_desc), mode, device.onMode)
        }
        Section(stringResource(R.string.device_notifications_title), YIcons.Bell, "device-notifications") {
            NoteText(stringResource(R.string.device_notifications_intro), Modifier.padding(horizontal = 12.dp, vertical = 8.dp))
            val allowed = notifications == NotificationState.Allowed
            StatusLine(
                stringResource(if (allowed) R.string.device_notifications_allowed else R.string.device_notifications_blocked),
                Modifier.padding(horizontal = 12.dp),
                error = !allowed,
                tag = "notifications-state",
            )
            Row(Modifier.padding(12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (!allowed) {
                    OutlineButton(
                        stringResource(R.string.device_notifications_allow),
                        onClick = { device.system.requestNotifications { notifications = device.system.notificationState() } },
                        tag = "notifications-allow",
                    )
                }
                OutlineButton(
                    stringResource(R.string.device_notifications_open_settings),
                    onClick = device.system::openNotificationSettings,
                    tag = "notifications-open-settings",
                )
            }
            if (!allowed) {
                NoteText(stringResource(R.string.device_notifications_denied_hint), Modifier.padding(start = 12.dp, end = 12.dp, bottom = 10.dp))
            }
        }
        Section(stringResource(R.string.device_battery_title), YIcons.Bolt, "device-battery") {
            NoteText(stringResource(R.string.device_battery_intro), Modifier.padding(horizontal = 12.dp, vertical = 8.dp))
            StatusLine(
                stringResource(if (unrestricted) R.string.device_battery_unrestricted else R.string.device_battery_optimized),
                Modifier.padding(horizontal = 12.dp),
                tag = "battery-state",
            )
            if (!unrestricted) {
                OutlineButton(
                    stringResource(R.string.device_battery_request),
                    onClick = device.system::requestIgnoreBatteryOptimizations,
                    modifier = Modifier.padding(12.dp),
                    tag = "battery-request",
                )
            } else {
                Spacer(Modifier.height(10.dp))
            }
        }
        SettingsCard {
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

@Composable
private fun Section(title: String, icon: androidx.compose.ui.graphics.vector.ImageVector, tag: String, content: @Composable () -> Unit) {
    val c = Yhtye.colors
    SettingsCard(Modifier.testTag(tag)) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            YIcon(icon, c.tx2, 16.dp)
            Text(title, color = c.tx, fontSize = 14.sp)
        }
        HorizontalDivider(color = c.line)
        content()
    }
}

@Composable
private fun InfoRow(label: String, value: String, tag: String) {
    val c = Yhtye.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 40.dp).padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(label, color = c.tx3, fontSize = 13.sp)
        Text(value, color = c.tx, fontFamily = MonoFont, fontSize = 12.5.sp, modifier = Modifier.weight(1f).testTag(tag), textAlign = androidx.compose.ui.text.style.TextAlign.End)
    }
}

@Composable
private fun ModeRow(mode: BackgroundMode, title: String, description: String, selected: BackgroundMode, onSelect: (BackgroundMode) -> Unit) {
    val c = Yhtye.colors
    val on = mode == selected
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .clickable(role = Role.RadioButton) { onSelect(mode) }
            .padding(horizontal = 4.dp, vertical = 6.dp)
            .semantics { this.selected = on }
            .testTag("mode-${mode.wire}"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(
            selected = on,
            onClick = null,
            colors = RadioButtonDefaults.colors(selectedColor = c.accent, unselectedColor = c.tx4),
            modifier = Modifier.padding(horizontal = 8.dp),
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, color = c.tx, fontSize = 14.sp)
            Text(description, color = c.tx3, fontSize = 12.5.sp, lineHeight = 19.sp)
        }
    }
}

@Composable
private fun connectionText(state: ConnectionState): String = stringResource(
    when (state) {
        is ConnectionState.Connected -> R.string.device_state_connected
        is ConnectionState.Connecting -> R.string.device_state_connecting
        is ConnectionState.WaitingToReconnect -> R.string.device_state_waiting
        ConnectionState.Idle -> R.string.device_state_idle
        ConnectionState.Revoked -> R.string.device_state_revoked
        is ConnectionState.Rejected -> R.string.state_rejected
    },
)
