package io.github.tom039224.yhtye.ui.conversation

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.client.RpcException
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.deleteBlocker
import io.github.tom039224.yhtye.shared.sync.describe
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.launch

// 会話の上部バーのメニュー (PC のチャットの行の「⋯」と、コンポーザの下の ContextBar をスマホではここに集めた):
// 文脈の使用量と圧縮 (`/compact`)、チャットの名前の変更 (タイトルがその場で入力欄になる)、削除 (確認あり)。

/** この割合以上でメーターを警告・危険の色にする (PC の ContextBar)。 */
private const val WARN_PERCENT = 70
private const val HIGH_PERCENT = 90

/** @param onRename 名前の変更を始める (上部バーのタイトルが入力欄になる。PC の行の中の入力欄と同じ)。 */
@Composable
fun ChatMenu(model: ConversationModel, sync: AppStore, onRename: () -> Unit) {
    val c = Yhtye.colors
    val chat = model.chat
    var open by remember { mutableStateOf(false) }
    var confirmDelete by rememberSaveable { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val label = "${model.title} の操作"
    Box {
        IconButton(
            onClick = { open = true },
            enabled = chat != null,
            modifier = Modifier.semantics { contentDescription = label }.testTag("chat-menu"),
        ) { YIcon(YIcons.More, if (chat != null) c.tx2 else c.tx5, 20.dp) }
        DropdownMenu(expanded = open && chat != null, onDismissRequest = { open = false }) {
            ContextMeter(model)
            val compact = model.compact
            DropdownMenuItem(
                text = {
                    Column {
                        Text(if (compact == CompactState.Compacting) "圧縮中…" else "コンテキストを圧縮")
                        Text(
                            when (compact) {
                                is CompactState.Disabled -> compact.reason
                                CompactState.Compacting -> "コンテキストを圧縮しています…"
                                CompactState.Ready -> "/compact を送信"
                            },
                            color = c.tx4,
                            fontSize = 12.sp,
                            lineHeight = 16.sp,
                        )
                    }
                },
                leadingIcon = { YIcon(if (compact == CompactState.Compacting) YIcons.Loader else YIcons.Minimize, c.tx3, 18.dp, spin = compact == CompactState.Compacting) },
                enabled = compact == CompactState.Ready,
                onClick = {
                    open = false
                    scope.launch { sync.compactChat() }
                },
                modifier = Modifier.testTag("menu-compact"),
            )
            HorizontalDivider(color = c.line)
            DropdownMenuItem(
                text = { Text("名前を変更") },
                leadingIcon = { YIcon(YIcons.Edit, c.tx3, 18.dp) },
                onClick = {
                    open = false
                    onRename()
                },
                modifier = Modifier.testTag("menu-rename"),
            )
            DropdownMenuItem(
                text = { Text("削除", color = c.handFg) },
                leadingIcon = { YIcon(YIcons.Trash, c.handBar, 18.dp) },
                onClick = {
                    open = false
                    confirmDelete = true
                },
                modifier = Modifier.testTag("menu-delete"),
            )
        }
    }
    if (chat != null && confirmDelete) {
        val blocker = model.project?.let { deleteBlocker(it, chat.id) }
        DeleteDialog(model.title, blocker, onDismiss = { confirmDelete = false }) { sync.deleteChat(chat.id) }
    }
}

/** 文脈の使用量 (PC の ContextBar のメーター)。 */
@Composable
private fun ContextMeter(model: ConversationModel) {
    val c = Yhtye.colors
    val usage = model.usage
    val pct = usage?.let(::usagePercent)
    val tone = when {
        pct == null -> c.tx4
        pct >= HIGH_PERCENT -> c.handBar
        pct >= WARN_PERCENT -> c.accent
        else -> c.doneBar
    }
    val description = if (usage == null) {
        "コンテキスト使用量: 未取得 (オーケストレータが報告すると表示されます)"
    } else {
        buildString {
            append("コンテキスト使用量: ${usage.used.toLong()} / ${if (usage.size > 0) usage.size.toLong() else "不明"} トークン")
            if (pct != null) append(" ($pct%)")
            usage.cost?.let { append("\n累計コスト: ${"%.2f".format(it.amount)} ${it.currency}") }
        }
    }
    Column(
        Modifier
            .padding(horizontal = 16.dp, vertical = 10.dp)
            .width(240.dp)
            .semantics { contentDescription = description; liveRegion = LiveRegionMode.Polite }
            .testTag("context-meter"),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text("コンテキスト", color = c.tx3, fontSize = 12.sp)
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(1f).height(4.dp).clip(RoundedCornerShape(2.dp)).background(c.lineCard)) {
                Box(Modifier.fillMaxWidth((pct ?: 0) / 100f).fillMaxHeight().background(tone))
            }
            Text(meterText(usage), color = c.tx2, fontFamily = MonoFont, fontSize = 12.sp, modifier = Modifier.padding(start = 10.dp))
        }
        usage?.cost?.let {
            Text("累計コスト: ${"%.2f".format(it.amount)} ${it.currency}", color = c.tx4, fontFamily = MonoFont, fontSize = 11.sp)
        }
    }
}

/**
 * 削除の前に聞く (PC の DeleteConfirm): 何が消えて何が残るか。今は削除できなければ理由だけを出す。コアが断った理由も出す。
 * 表示中のチャットを消したら、同期層が別のチャットを出す。
 */
@Composable
private fun DeleteDialog(title: String, blocker: String?, onDismiss: () -> Unit, delete: suspend () -> Unit) {
    val c = Yhtye.colors
    val scope = rememberCoroutineScope()
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(if (blocker == null) "チャットの削除" else "削除できません") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (blocker == null) {
                    Text("「$title」チャットを削除しますか?", color = c.tx, fontWeight = FontWeight.SemiBold)
                    Text("会話の記録は消え、作業ツリーとブランチは残ります。", color = c.tx3)
                }
                (blocker ?: error)?.let {
                    Text(it, color = c.handFg, modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite }.testTag("delete-error"))
                }
            }
        },
        confirmButton = {
            if (blocker == null) {
                TextButton(
                    onClick = {
                        busy = true
                        error = null
                        scope.launch {
                            try {
                                delete()
                                onDismiss()
                            } catch (e: RpcException) {
                                error = describe(e)
                                busy = false
                            }
                        }
                    },
                    enabled = !busy,
                    modifier = Modifier.testTag("delete-confirm"),
                ) { Text("削除する", color = c.handBar) }
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss, enabled = !busy) { Text(if (blocker == null) "キャンセル" else "閉じる") }
        },
    )
}
