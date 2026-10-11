package io.github.tom039224.yhtye.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.runtime.remember
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.session.Session
import io.github.tom039224.yhtye.ui.settings.DevicePane
import io.github.tom039224.yhtye.ui.settings.SystemAccess
import io.github.tom039224.yhtye.ui.settings.rememberSystemAccess

/** 画面の切り替え: 読み込み中 → ペアリング前 / ペアリング後 (3 パネルのホーム)。 */
@Composable
fun YhtyeRoot(session: Session) {
    val phase by session.phase.collectAsStateWithLifecycle()
    val incoming by session.incoming.collectAsStateWithLifecycle()
    val openChat by session.openChat.collectAsStateWithLifecycle()
    val system = rememberSystemAccess()
    when (val current = phase) {
        Session.Phase.Loading -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            CircularProgressIndicator()
        }
        is Session.Phase.Unpaired -> {
            // ペアリングしていないので、通知のタップで頼まれたチャットは開けない
            LaunchedEffect(openChat) { openChat?.let(session::consumeOpenChat) }
            PairingFlow(session, current.revoked, incoming)
        }
        is Session.Phase.Paired -> {
            AskNotificationsOnce(session, system)
            val device = remember(current.credentials, system) {
                DevicePane(current.credentials, session.device.mode, session.device::setMode, system)
            }
            MainScreen(
                deviceId = current.credentials.deviceId,
                sync = current.sync,
                onReconnect = current.client::start,
                onUnpair = session::unpair,
                device = device,
                openChat = openChat,
                onOpenChatHandled = session::consumeOpenChat,
            )
            when (incoming) {
                is Session.Incoming.Target -> RepairDialog(
                    onConfirm = session::unpair, // ペアリング前の画面が受け取ったリンクを使う
                    onDismiss = session::consumeIncoming,
                )
                // ペアリング済みなら読めないリンクは無視する
                is Session.Incoming.Invalid -> LaunchedEffect(incoming) { session.consumeIncoming() }
                null -> Unit
            }
        }
    }
}

/**
 * 通知の許可 (Android 13 以降) を、ペアリング後の最初のホームで 1 回だけ尋ねる。許可が無いと裏で作業が終わっても
 * 知らせられないため。断られたあとは設定画面の「この端末」から (OS の画面が出なくなっていれば OS の設定へ)。
 */
@Composable
private fun AskNotificationsOnce(session: Session, system: SystemAccess) {
    LaunchedEffect(Unit) {
        if (session.device.notificationsAsked) return@LaunchedEffect
        session.device.notificationsAsked = true
        if (system.needsNotificationPermission()) system.requestNotifications()
    }
}

@Composable
private fun RepairDialog(onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.repair_title)) },
        text = { Text(stringResource(R.string.repair_message)) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.repair_confirm)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}
