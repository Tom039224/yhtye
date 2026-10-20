package io.github.tom039224.yhtye.ui.conversation

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.ThisApp
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.attachments.UriAttachments
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.Place
import io.github.tom039224.yhtye.shared.sync.of
import io.github.tom039224.yhtye.shared.version.AppVersion
import io.github.tom039224.yhtye.shared.version.Compatibility
import io.github.tom039224.yhtye.shared.version.compatibility
import io.github.tom039224.yhtye.ui.common.EmptyState
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.common.breatheAlpha
import kotlinx.coroutines.launch

private val TOPBAR_HEIGHT = 56.dp
private val WAIT_SHAPE = RoundedCornerShape(10.dp)
private val PILL_SHAPE = RoundedCornerShape(10.dp)
private const val MILLIS_PER_SECOND = 1_000L

/**
 * 中央 = オーケストレータとの会話だけ (モックの中央、PC の Conversation)。状態は同期層 ([app]) から読み、操作も
 * 同期層 ([sync]) に送る (楽観的に変えない)。
 *
 * @param onOpenLeft ☰ (左パネル)。[showPanelButtons] が false (左右のパネルを常時表示) のときは出さない。
 * @param onOpenRight グループ (右パネル) の入口と、待機バナーのタップ。
 * @param appVersion このアプリの版 (PC の版と合わなければ接続のバナーと同じ場所に知らせる)。
 */
@Composable
fun ConversationScreen(
    app: AppState,
    sync: AppStore,
    mentions: MentionsState,
    showPanelButtons: Boolean,
    onOpenLeft: () -> Unit,
    onOpenRight: () -> Unit,
    onReconnect: () -> Unit,
    modifier: Modifier = Modifier,
    appVersion: AppVersion = ThisApp.version,
) {
    val model = conversationModel(app)
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val uriAttachments = remember(context) { UriAttachments(context.contentResolver) }
    val drafts by sync.attachments.state.collectAsStateWithLifecycle()
    val chat = model.chat
    val project = model.project
    Column(modifier.fillMaxSize().background(Yhtye.colors.bgCenter)) {
        TopBar(model, showPanelButtons, onOpenLeft, onOpenRight, sync)
        ConnectionBanner(app, appVersion, onReconnect, onDismissError = sync::dismissError)
        Box(Modifier.weight(1f).fillMaxWidth()) {
            when {
                project == null ->
                    if (app.busy.opening) {
                        EmptyState(YIcons.Loader, "読み込み中…", Modifier.align(Alignment.Center))
                    } else {
                        EmptyState(YIcons.Folder, NO_PROJECT_HINT, Modifier.align(Alignment.Center), testTag = "no-project")
                    }
                chat == null -> EmptyState(YIcons.MessagePlus, NO_CHAT_HINT, Modifier.align(Alignment.Center), testTag = "no-chat")
                model.isEmpty -> EmptyState(YIcons.Message, EMPTY_CONVERSATION, Modifier.align(Alignment.Center))
                else -> key(project.info.id, chat.id) {
                    TranscriptList(
                        model,
                        onLoadOlder = { scope.launch { sync.loadOlderHistory() } },
                        Modifier.fillMaxSize(),
                        loadAttachment = rememberLoadAttachment(sync, project.info.id, chat.id),
                    )
                }
            }
        }
        model.wait?.let { WaitBannerView(it, onClick = onOpenRight) }
        if (project != null && chat != null) {
            val projectId = project.info.id
            key(projectId, chat.id) {
                Composer(
                    initialText = sync.drafts.get(project.info.id, chat.id),
                    onTextChange = { sync.drafts.set(project.info.id, chat.id, it) },
                    disabledReason = model.disabledReason,
                    sending = model.sending,
                    turnRunning = model.turnRunning,
                    starting = model.starting,
                    mentions = mentions.of(chat.id),
                    onRemoveMention = { mentions.remove(chat.id, it) },
                    onSend = { text ->
                        val ok = sync.sendMessage(withMentions(text, mentions.of(chat.id)))
                        if (ok) mentions.clear(chat.id)
                        ok
                    },
                    onCancel = { scope.launch { sync.cancelTurn() } },
                    modifier = Modifier.windowInsetsPadding(WindowInsets.ime.union(WindowInsets.navigationBars)),
                    attachments = drafts.of(projectId, chat.id),
                    loadAttachment = rememberLoadAttachment(sync, projectId, chat.id),
                    onAddFiles = { uris -> scope.launch { uriAttachments.addTo(sync.attachments, projectId, chat.id, uris) } },
                    onRemoveAttachment = { sync.attachments.remove(projectId, chat.id, it) },
                )
            }
        } else {
            Spacer(Modifier.windowInsetsPadding(WindowInsets.navigationBars))
        }
    }
}

/** [chat] の添付の中身を同期層のキャッシュから取る (会話の画像・コンポーザのサムネイル)。 */
@Composable
private fun rememberLoadAttachment(sync: AppStore, project: String, chat: String): LoadAttachment =
    remember(sync, project, chat) { LoadAttachment { sync.attachmentPreviews.load(project, chat, it) } }

@Composable
private fun TopBar(
    model: ConversationModel,
    showPanelButtons: Boolean,
    onOpenLeft: () -> Unit,
    onOpenRight: () -> Unit,
    sync: AppStore,
) {
    val c = Yhtye.colors
    // 名前の変更中はタイトルが入力欄になる (チャットが変わったら終わる)
    var renaming by rememberSaveable(model.chat?.id) { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth().windowInsetsPadding(WindowInsets.statusBars)) {
        Row(
            Modifier.fillMaxWidth().height(TOPBAR_HEIGHT).padding(horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (showPanelButtons) {
                IconButton(onClick = onOpenLeft, modifier = Modifier.semantics { contentDescription = "プロジェクトとチャット" }.testTag("open-left")) {
                    YIcon(YIcons.Menu, c.tx2, 20.dp)
                }
            } else {
                Spacer(Modifier.width(12.dp))
            }
            val chat = model.chat
            if (renaming && chat != null) {
                Box(Modifier.weight(1f).padding(start = 2.dp)) {
                    RenameField(chat.title.orEmpty(), onDone = { renaming = false }) { sync.renameChat(chat.id, it) }
                }
            } else {
                Column(Modifier.weight(1f).padding(start = 2.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text(
                        model.title,
                        color = c.tx,
                        fontSize = 16.sp,
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.testTag("chat-title"),
                    )
                    model.placeText?.let { BranchPill(it, missing = model.place == Place.Missing) }
                }
                if (chat != null) SessionBadgeView(model.badge)
                if (showPanelButtons) {
                    IconButton(onClick = onOpenRight, modifier = Modifier.semantics { contentDescription = "タスク" }.testTag("open-right")) {
                        YIcon(YIcons.List, c.tx2, 20.dp)
                    }
                }
                ChatMenu(model, sync, onRename = { renaming = true })
            }
        }
        HorizontalDivider(color = c.line)
    }
}

/** 作業ツリーのブランチの小さなピル (見つからなければ警告のアイコン)。 */
@Composable
private fun BranchPill(text: String, missing: Boolean) {
    val c = Yhtye.colors
    Row(
        Modifier
            .height(20.dp)
            .clip(PILL_SHAPE)
            .border(1.dp, c.lineCard, PILL_SHAPE)
            .padding(horizontal = 7.dp)
            .testTag("chat-branch"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        YIcon(if (missing) YIcons.Alert else YIcons.Branch, if (missing) c.handBar else c.tx3, 12.dp)
        Text(
            text,
            color = c.tx3,
            fontFamily = MonoFont,
            fontSize = 11.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(start = 4.dp),
        )
    }
}

/** オーケストレータの状態を形でも示す (中を抜いた丸 = 未起動、緑 = 待機、青 + 光輪 = 作業中)。言葉は読み上げ用。 */
@Composable
private fun SessionBadgeView(badge: SessionBadge) {
    val c = Yhtye.colors
    Box(
        Modifier.size(40.dp, 48.dp).semantics { contentDescription = "オーケストレータ: ${badge.label}" }.testTag("orchestrator-status"),
        contentAlignment = Alignment.Center,
    ) {
        when (badge) {
            SessionBadge.Starting -> YIcon(YIcons.Loader, c.tx3, 16.dp, spin = true)
            is SessionBadge.Bad -> YIcon(YIcons.Alert, c.handBar, 16.dp)
            is SessionBadge.Idle -> Box(Modifier.size(10.dp).border(1.5.dp, c.tx4, CircleShape))
            SessionBadge.Ready -> Box(Modifier.size(10.dp).background(c.doneBar, CircleShape))
            SessionBadge.Working -> Box(
                Modifier
                    .alpha(breatheAlpha(periodMs = 2_000))
                    .size(16.dp)
                    .background(c.implBg, CircleShape)
                    .padding(3.dp)
                    .background(c.implBar, CircleShape),
            )
        }
    }
}

/** 会話の末尾のグループ待機バナー。タップで右パネル (グループ) を開く。 */
@Composable
private fun WaitBannerView(wait: WaitBanner, onClick: () -> Unit) {
    val c = Yhtye.colors
    Row(
        Modifier
            .padding(start = 12.dp, end = 12.dp, bottom = 6.dp)
            .fillMaxWidth()
            .clip(WAIT_SHAPE)
            .background(c.bgInset)
            .dashedBorder(c.lineDashed, 10.dp)
            .clickable(role = Role.Button, onClickLabel = "グループを開く", onClick = onClick)
            .padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 9.dp)
            .testTag("wait-banner"),
    ) {
        val icon = when (wait.kind) {
            WaitBanner.Kind.Finishing -> YIcons.Merge
            WaitBanner.Kind.Awaiting -> YIcons.Hourglass
            WaitBanner.Kind.Waiting -> YIcons.Layers
        }
        val busy = wait.kind == WaitBanner.Kind.Finishing
        YIcon(icon, if (busy) c.revBar else c.tx4, 14.dp, Modifier.padding(top = 3.dp).alpha(if (busy) breatheAlpha() else 1f))
        Column(Modifier.weight(1f).padding(start = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(wait.text, color = c.tx3, fontSize = 12.5.sp, lineHeight = 19.sp)
            Box(Modifier.fillMaxWidth().height(3.dp).clip(RoundedCornerShape(2.dp)).background(c.lineCard)) {
                Box(Modifier.fillMaxWidth(wait.share / 100f).fillMaxHeight().background(c.doneBar))
            }
        }
    }
}

/**
 * 接続が切れているとき・画面に出す失敗 (PC の ConnectionBanner。黙って失敗しない)。再接続を待っている間は
 * 「今すぐ再接続」(T-61 のホームと同じ)。PC の版とこの端末の版が合わないときも (PC の版が分からない古い PC には出さない)。
 */
@Composable
private fun ConnectionBanner(app: AppState, appVersion: AppVersion, onReconnect: () -> Unit, onDismissError: (Long) -> Unit) {
    val c = Yhtye.colors
    val connection = app.connection
    val lost: Pair<String, String?>? = when (connection) {
        is ConnectionState.WaitingToReconnect -> {
            val seconds = (connection.delayMs + MILLIS_PER_SECOND - 1) / MILLIS_PER_SECOND
            val reason = connection.reason?.let { ": $it" }.orEmpty()
            "コアとの接続が切れました$reason · $seconds 秒後に再接続" to stringResource(R.string.state_reconnect_now)
        }
        is ConnectionState.Connecting -> if (connection.attempt > 0) stringResource(R.string.state_connecting) to null else null
        is ConnectionState.Rejected -> stringResource(R.string.state_rejected) to stringResource(R.string.retry)
        else -> null
    }
    val mismatch = compatibility(app.host.version, appVersion) as? Compatibility.Incompatible
    if (lost == null && mismatch == null && app.errors.isEmpty()) return
    Column(Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        mismatch?.let {
            Alert(YIcons.Alert, stringResource(R.string.version_mismatch, it.pc.toString(), it.device.toString()), testTag = "version-mismatch") {}
        }
        lost?.let { (text, action) ->
            Alert(YIcons.Unplug, text, testTag = "connection-lost") {
                if (action != null) {
                    Text(
                        action,
                        color = c.accent,
                        fontSize = 13.sp,
                        fontWeight = FontWeight.SemiBold,
                        modifier = Modifier.clickable(role = Role.Button, onClick = onReconnect).padding(8.dp),
                    )
                }
            }
        }
        app.errors.forEach { e ->
            Alert(YIcons.Alert, e.message, testTag = "app-error") {
                IconButton(onClick = { onDismissError(e.id) }, modifier = Modifier.semantics { contentDescription = "閉じる" }) {
                    YIcon(YIcons.X, c.tx3, 14.dp)
                }
            }
        }
    }
}

@Composable
private fun Alert(icon: androidx.compose.ui.graphics.vector.ImageVector, text: String, testTag: String, action: @Composable () -> Unit) {
    val c = Yhtye.colors
    val shape = RoundedCornerShape(10.dp)
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(c.handBg.copy(alpha = 0.35f))
            .border(1.dp, c.handBar.copy(alpha = 0.5f), shape)
            .padding(start = 10.dp, end = 2.dp, top = 4.dp, bottom = 4.dp)
            .testTag(testTag),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        YIcon(icon, c.handBar, 14.dp)
        Text(text, color = c.tx, fontSize = 13.sp, lineHeight = 19.sp, modifier = Modifier.weight(1f).padding(horizontal = 8.dp, vertical = 6.dp))
        action()
    }
}
