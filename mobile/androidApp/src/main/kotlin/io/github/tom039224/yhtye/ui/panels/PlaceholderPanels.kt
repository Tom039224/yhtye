package io.github.tom039224.yhtye.ui.panels

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.byRecency
import io.github.tom039224.yhtye.shared.sync.chatRing
import io.github.tom039224.yhtye.shared.sync.groupLabel
import io.github.tom039224.yhtye.shared.sync.projectRing
import io.github.tom039224.yhtye.shared.sync.scopeState
import io.github.tom039224.yhtye.shared.sync.shortAge
import io.github.tom039224.yhtye.shared.sync.taskTone
import io.github.tom039224.yhtye.ui.common.EmptyState
import io.github.tom039224.yhtye.ui.common.RingMark
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.conversation.Mention
import io.github.tom039224.yhtye.ui.conversation.NEW_CHAT_TITLE
import io.github.tom039224.yhtye.ui.conversation.toneColor
import io.github.tom039224.yhtye.ui.conversation.toneIcon
import kotlinx.coroutines.launch

// 骨格の確認用の仮の左右パネル。T-69 が本番 (左: ビュー切り替えのレール・プロジェクトのドロップダウン・
// バックグラウンドで動作中のプロジェクト・ブランチとチャットの木・下端のホストと使用量、右: PC のタスク列) に差し替える。

private val ROW_SHAPE = RoundedCornerShape(10.dp)

/**
 * 仮の左パネル: プロジェクトの一覧 (タップで開く)、表示中のプロジェクトのチャット (タップで出す)、新しいチャット、
 * 下端に接続先とペアリング解除 (スマホの設定画面ができるまで。T-71)。
 */
@Composable
fun LeftPanelPlaceholder(app: AppState, sync: AppStore, now: Long, onChatChosen: () -> Unit, onUnpair: () -> Unit) {
    val c = Yhtye.colors
    val scope = rememberCoroutineScope()
    val view = app.project
    var confirmUnpair by rememberSaveable { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars)) {
        LazyColumn(Modifier.weight(1f).testTag("left-panel")) {
            item { SectionLabel("Project") }
            if (!app.projectsLoaded) item { EmptyState(YIcons.Loader, "読み込み中…") }
            items(app.projects.sortedBy { it.name.lowercase() }, key = { "p-${it.id}" }) { p ->
                PanelRow(
                    selected = p.id == view?.info?.id,
                    onClick = { if (p.id != view?.info?.id) scope.launch { sync.openProject(p.path) } },
                    tag = "project-${p.id}",
                ) {
                    RingMark(projectRing(p, view))
                    Text(p.name, color = c.tx, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                }
            }
            if (view != null) {
                item { SectionLabel("チャット") }
                items(byRecency(view.chats), key = { "c-${it.id}" }) { chat ->
                    PanelRow(
                        selected = chat.id == view.selectedChat,
                        onClick = {
                            sync.selectChat(chat.id)
                            onChatChosen()
                        },
                        tag = "chat-${chat.id}",
                    ) {
                        RingMark(chatRing(view, chat.id))
                        Text(
                            chat.title ?: NEW_CHAT_TITLE,
                            color = c.tx2,
                            fontSize = 14.sp,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        if (chat.id in view.unread) {
                            Box(Modifier.size(7.dp).background(c.accent, CircleShape).semantics { contentDescription = "未読の通知" })
                        }
                        Text(shortAge(now, chat.lastUsedMs), color = c.tx4, fontFamily = MonoFont, fontSize = 11.sp)
                    }
                }
                item {
                    // メインの作業ツリー (git を読む前はプロジェクトのパス = 元のクローン) に作る
                val mainWorktree = app.git?.overview?.worktrees?.firstOrNull { it.isMain }?.path ?: view.info.path
                PanelRow(selected = false, onClick = { scope.launch { if (sync.createChat(worktree = mainWorktree)) onChatChosen() } }, tag = "new-chat") {
                        YIcon(YIcons.MessagePlus, c.tx3, 16.dp)
                        Text(NEW_CHAT_TITLE, color = c.tx3, fontSize = 14.sp)
                    }
                }
            }
        }
        HorizontalDivider(color = c.line)
        Row(
            Modifier
                .fillMaxWidth()
                .background(c.bgRail)
                .windowInsetsPadding(WindowInsets.navigationBars)
                .padding(start = 14.dp, end = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val connected = app.connection is ConnectionState.Connected
            Box(
                Modifier
                    .size(7.dp)
                    .background(if (connected) c.doneBar else c.ringStop, CircleShape)
                    .semantics { contentDescription = if (connected) "接続中" else "未接続" },
            )
            Text(
                app.host.name ?: "—",
                color = c.tx2,
                fontFamily = MonoFont,
                fontSize = 11.5.sp,
                modifier = Modifier.weight(1f).padding(start = 7.dp),
            )
            TextButton(onClick = { confirmUnpair = true }, modifier = Modifier.testTag("unpair")) {
                Text(stringResource(R.string.home_unpair), fontSize = 12.sp, color = c.tx3)
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

/** 仮の右パネル: 選択中のチャットのグループとタスク (`@` でコンポーザに引用を積む。PC のタスクカードの `@`)。 */
@Composable
fun RightPanelPlaceholder(app: AppState, onMention: (Mention) -> Unit) {
    val c = Yhtye.colors
    val view = app.project
    val state = view?.state?.let { scopeState(it, view.selectedChat) }
    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars)) {
        Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            YIcon(YIcons.List, c.tx3, 16.dp)
            Text("tasks", color = c.tx3, fontFamily = MonoFont, fontSize = 12.sp, modifier = Modifier.padding(start = 8.dp))
        }
        HorizontalDivider(color = c.line)
        if (state == null || state.groups.isEmpty()) {
            EmptyState(
                YIcons.List,
                if (view?.selectedChat != null) "グループはまだありません" else "チャットを選ぶとタスクが出ます",
                testTag = "no-groups",
            )
            return@Column
        }
        LazyColumn(Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars).testTag("right-panel")) {
            items(state.groups.reversed(), key = { it.id }) { g ->
                Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(g.title, color = c.tx, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                        Text("${g.id} · ${groupLabel(state, g)}", color = c.tx3, fontFamily = MonoFont, fontSize = 11.5.sp)
                    }
                    state.tasks.filter { it.group == g.id }.forEach { t ->
                        val tone = taskTone(t)
                        Row(Modifier.fillMaxWidth().heightIn(min = 44.dp), verticalAlignment = Alignment.CenterVertically) {
                            YIcon(toneIcon(tone, t.kind), toneColor(tone), 14.dp)
                            Text(t.id, color = c.tx3, fontFamily = MonoFont, fontSize = 11.5.sp, modifier = Modifier.padding(horizontal = 8.dp))
                            Text(t.title, color = c.txBody, fontSize = 14.sp, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                            IconButton(
                                onClick = { onMention(Mention(t.id, t.title)) },
                                modifier = Modifier.semantics { contentDescription = "${t.id} をオーケストレータへ引用" }.testTag("mention-button-${t.id}"),
                            ) { YIcon(YIcons.At, c.tx3, 18.dp) }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SectionLabel(text: String) {
    Text(
        text,
        color = Yhtye.colors.tx4,
        fontSize = 12.sp,
        modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 14.dp, bottom = 6.dp),
    )
}

@Composable
private fun PanelRow(selected: Boolean, onClick: () -> Unit, tag: String, content: @Composable () -> Unit) {
    val c = Yhtye.colors
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 8.dp, vertical = 1.dp)
            .clip(ROW_SHAPE)
            .then(if (selected) Modifier.background(c.bgSelected) else Modifier)
            .clickable(role = Role.Button, onClick = onClick)
            .heightIn(min = 44.dp)
            .padding(horizontal = 10.dp)
            .testTag(tag),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) { content() }
}
