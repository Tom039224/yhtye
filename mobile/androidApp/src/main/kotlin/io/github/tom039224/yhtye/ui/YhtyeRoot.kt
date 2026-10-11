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
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.session.Session

/** 画面の切り替え: 読み込み中 → ペアリング前 / ペアリング後 (ホーム)。 */
@Composable
fun YhtyeRoot(session: Session) {
    val phase by session.phase.collectAsStateWithLifecycle()
    val incoming by session.incoming.collectAsStateWithLifecycle()
    when (val current = phase) {
        Session.Phase.Loading -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            CircularProgressIndicator()
        }
        is Session.Phase.Unpaired -> PairingFlow(session, current.revoked, incoming)
        is Session.Phase.Paired -> {
            HomeScreen(current.credentials, current.client, current.sync, onUnpair = session::unpair)
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
