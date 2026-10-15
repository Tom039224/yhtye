package io.github.tom039224.yhtye.ui.panels

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.BranchState
import io.github.tom039224.yhtye.shared.api.generated.BranchStatuses
import io.github.tom039224.yhtye.shared.api.generated.GitOverview
import io.github.tom039224.yhtye.shared.client.RpcException
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.LoadPhase
import io.github.tom039224.yhtye.shared.sync.ProjectView
import io.github.tom039224.yhtye.shared.sync.WorktreeNode
import io.github.tom039224.yhtye.shared.sync.buildTree
import io.github.tom039224.yhtye.shared.sync.chatRing
import io.github.tom039224.yhtye.shared.sync.describe
import io.github.tom039224.yhtye.shared.sync.isInternalBranch
import io.github.tom039224.yhtye.shared.sync.shortAge
import io.github.tom039224.yhtye.shared.sync.statusLabel
import io.github.tom039224.yhtye.shared.sync.statusOf
import io.github.tom039224.yhtye.ui.common.ReorderMenu
import io.github.tom039224.yhtye.ui.common.RingMark
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.rememberActionScope
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.conversation.NEW_CHAT_TITLE
import kotlinx.coroutines.launch

// ブランチ (作業ツリー) とチャットの木 (PC の BranchTree / ChatRow、orchestrator-desktop §9、モックの「Branch」):
// 作業ツリーを今のブランチの名前で (左のアイコンは PR / マージの状態、PC と同じアイコン・色・文言)、折りたためて、
// それぞれに新しいチャット。見つからない作業ツリーのチャット、作業ツリーの無い他のブランチ、「+」で新しいブランチ。
// チャットの名前の変更・削除はスマホでは会話の上部のメニュー (決定事項)。

private val ROW_SHAPE = RoundedCornerShape(10.dp)
private val FORM_SHAPE = RoundedCornerShape(12.dp)
private val FIELD_SHAPE = RoundedCornerShape(8.dp)
private val STATUS_ICON_SIZE = 14.dp

/** 開始点の「HEAD」(PC の HEAD_OPTION)。 */
private const val HEAD_OPTION = ""

/** パネルを閉じても残す木の開閉 (隠れたパネルは合成しないので)。プロジェクトごと。 */
@Stable
class LeftPanelMemory {
    /** 作業ツリーごとに明示した開閉 (既定は「チャットがあるかメインクローンなら開く」)。 */
    private val toggled = mutableStateMapOf<Pair<String, String>, Boolean>()
    private val othersOpen = mutableStateMapOf<String, Boolean>()

    fun isOpen(project: String, w: WorktreeNode): Boolean = toggled[project to w.path] ?: (w.chats.isNotEmpty() || w.isMain)

    fun toggle(project: String, w: WorktreeNode) {
        toggled[project to w.path] = !isOpen(project, w)
    }

    fun othersOpen(project: String): Boolean = othersOpen[project] ?: false

    fun toggleOthers(project: String) {
        othersOpen[project] = !othersOpen(project)
    }
}

@Composable
fun BranchTree(app: AppState, sync: AppStore, now: Long, memory: LeftPanelMemory, onChatChosen: () -> Unit) {
    val c = Yhtye.colors
    val scope = rememberActionScope()
    val view = app.project
    val git = app.git?.takeIf { view != null && it.project == view.info.id }
    val overview = git?.overview
    var formOpen by rememberSaveable(view?.info?.id) { mutableStateOf(false) }
    val ready = view != null && (view.phase == LoadPhase.Ready || view.state != null)
    val order = app.branchOrder?.takeIf { view != null && it.project == view.info.id }?.keys.orEmpty()
    val tree = buildTree(view?.chats.orEmpty(), overview, git?.awaiting.orEmpty(), order)
    // 並べ替えは画面に出ている順を送る: 作業ツリーと、作業ツリーの無いブランチは別々の一覧
    val worktreeKeys = tree.worktrees.map { it.path }
    val moveBranches = { next: List<String> -> scope.launch { sync.reorderBranches(next) }; Unit }
    val newChat = { worktree: String?, branch: String? ->
        scope.launch { if (sync.createChat(worktree = worktree, branch = branch)) onChatChosen() }
        Unit
    }
    Column(Modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 6.dp, top = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            YIcon(YIcons.Branch, c.tx4, 14.dp)
            Text("Branch", color = c.tx4, fontSize = 12.sp, modifier = Modifier.weight(1f).padding(start = 6.dp))
            IconButton(
                onClick = { formOpen = !formOpen },
                enabled = ready,
                modifier = Modifier
                    .semantics {
                        contentDescription = "新しいブランチ"
                        stateDescription = if (formOpen) "開いている" else "閉じている"
                    }
                    .testTag("new-branch"),
            ) { YIcon(YIcons.Plus, if (ready) c.tx3 else c.tx5, 16.dp) }
        }
        if (formOpen && view != null) {
            NewBranchForm(overview) { name, from ->
                sync.createBranch(name, from)
                formOpen = false
                onChatChosen()
            }
        }
        when {
            view == null -> SideNote("プロジェクトを開くと表示します。")
            overview == null && git?.error != null -> SideNote(git.error.orEmpty(), error = true)
            tree.worktrees.isEmpty() && tree.lost.isEmpty() && tree.otherBranches.isEmpty() -> SideNote("作業ツリーはまだありません。")
        }
        if (view == null) return@Column
        val project = view.info.id
        val statuses = git?.statuses
        tree.worktrees.forEach { w ->
            val open = memory.isOpen(project, w)
            val branch = overview?.worktrees?.find { it.path == w.path }?.branch
            WorktreeRow(
                w, open, branch, statuses, worktreeKeys, moveBranches,
                onToggle = { memory.toggle(project, w) },
                onNewChat = { newChat(w.path, null) },
            )
            if (open) w.chats.forEach { chat -> ChatRow(view, chat, now, onChosen = onChatChosen, sync = sync) }
        }
        if (tree.lost.isNotEmpty()) {
            Row(
                Modifier.padding(start = 16.dp, top = 8.dp, bottom = 2.dp).semantics(mergeDescendants = true) {
                    contentDescription = "見つからない作業ツリーのチャット"
                }.testTag("lost-worktrees"),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                YIcon(YIcons.Alert, c.tx4, 12.dp)
                Text("見つからない作業ツリー", color = c.tx4, fontSize = 12.sp)
            }
            tree.lost.forEach { chat -> ChatRow(view, chat, now, onChosen = onChatChosen, sync = sync) }
        }
        if (tree.otherBranches.isNotEmpty()) {
            OtherBranches(
                tree.otherBranches, statuses, memory.othersOpen(project), moveBranches,
                onToggle = { memory.toggleOthers(project) },
            ) { newChat(null, it) }
        }
    }
}

@Composable
private fun SideNote(text: String, error: Boolean = false) {
    val c = Yhtye.colors
    Text(
        text,
        color = if (error) c.handFg else c.tx4,
        fontSize = 12.5.sp,
        lineHeight = 19.sp,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp).testTag("side-note"),
    )
}

/**
 * ブランチの左のアイコン: PR / マージの状態 (PC の StatusIcon と同じアイコン・色・日本語ラベル、Orca のサイドバーに倣う)。
 * PR が無ければブランチ、開いた PR は緑の PR、マージ済みは紫のマージ (その後に変更があれば小さな点)。
 * 既定のブランチ自身と detached の作業ツリーはラベルの無いブランチ。
 */
@Composable
private fun StatusIcon(branch: String?, statuses: BranchStatuses?, testTag: String? = null) {
    val c = Yhtye.colors
    val tag = if (testTag == null) Modifier else Modifier.testTag(testTag)
    val found = statusOf(branch, statuses)
    if (found == null) {
        YIcon(YIcons.Branch, c.tx4, STATUS_ICON_SIZE, tag)
        return
    }
    val (state, pr) = found
    val (icon, tint) = when (state) {
        BranchState.Unmerged -> YIcons.Branch to c.tx4
        BranchState.PrOpen -> YIcons.PullRequest to c.doneBar
        BranchState.Merged, BranchState.MergedWithChanges -> YIcons.Merge to c.revBar
        BranchState.Unknown, BranchState.Unrecognized -> YIcons.Help to c.tx5
    }
    Box(Modifier.semantics { contentDescription = statusLabel(state, pr) }.then(tag)) {
        YIcon(icon, tint, STATUS_ICON_SIZE, Modifier.testTag("branch-icon-${icon.name}"))
        if (state == BranchState.MergedWithChanges) {
            // パネルの地の縁で、マージのアイコンから切り抜いて見せる
            Box(
                Modifier.align(Alignment.TopEnd).offset(x = 2.dp, y = (-2).dp).size(7.dp)
                    .background(c.bgPanel, CircleShape).padding(1.dp).background(c.handBar, CircleShape)
                    .testTag("branch-status-badge"),
            )
        }
    }
}

@Composable
private fun WorktreeRow(
    node: WorktreeNode,
    open: Boolean,
    branch: String?,
    statuses: BranchStatuses?,
    keys: List<String>,
    onMove: (List<String>) -> Unit,
    onToggle: () -> Unit,
    onNewChat: () -> Unit,
) {
    val c = Yhtye.colors
    var menu by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Row(
            Modifier
                .weight(1f)
                .heightIn(min = 40.dp)
                .combinedClickable(role = Role.Button, onLongClick = { menu = true }, onClick = onToggle)
                .semantics {
                    stateDescription = if (open) "開いている" else "閉じている"
                    contentDescription = if (node.isMain) "${node.label} · メインクローン" else node.label
                }
                .padding(start = 12.dp, end = 4.dp)
                .testTag("worktree-${node.path}"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            YIcon(YIcons.ChevronDown, c.tx4, 14.dp, Modifier.rotate(if (open) 0f else -90f))
            StatusIcon(branch, statuses, testTag = if (node.isMain) "main-clone" else null)
            Text(
                node.label,
                color = c.tx2,
                fontFamily = MonoFont,
                fontSize = 12.5.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (node.missing) {
                Box(Modifier.semantics { contentDescription = "作業ツリーが見つかりません" }) { YIcon(YIcons.Alert, c.handBar, 12.dp) }
            }
            ReorderMenu(menu, { menu = false }, keys, node.path, "worktree-move-${node.path}", onMove)
        }
        if (!node.missing) {
            IconButton(
                onClick = onNewChat,
                modifier = Modifier.padding(end = 4.dp).semantics { contentDescription = "${node.label} の新しいチャット" }.testTag("new-chat-${node.path}"),
            ) { YIcon(YIcons.MessagePlus, c.tx4, 16.dp) }
        }
    }
}

/** チャットの行: リング・題・未読の点・経過。タップで中央に出して戻る。 */
@Composable
private fun ChatRow(view: ProjectView, chat: ChatInfo, now: Long, onChosen: () -> Unit, sync: AppStore) {
    val c = Yhtye.colors
    val selected = view.selectedChat == chat.id
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 8.dp, vertical = 1.dp)
            .clip(ROW_SHAPE)
            .then(if (selected) Modifier.background(c.bgSelected) else Modifier)
            .clickable(role = Role.Button) {
                sync.selectChat(chat.id)
                onChosen()
            }
            .semantics { this.selected = selected }
            .heightIn(min = 42.dp)
            .padding(start = 30.dp, end = 10.dp)
            .testTag("chat-${chat.id}"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        RingMark(chatRing(view, chat.id))
        Text(
            chat.title ?: NEW_CHAT_TITLE,
            color = if (selected) c.tx else c.tx2,
            fontSize = 14.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        if (chat.id in view.unread) {
            Box(Modifier.size(7.dp).background(c.accent, CircleShape).semantics { contentDescription = "未読の通知" }.testTag("unread-${chat.id}"))
        }
        Text(shortAge(now, chat.lastUsedMs), color = c.tx4, fontFamily = MonoFont, fontSize = 11.sp)
    }
}

/** 作業ツリーにチェックアウトされていない他のブランチ (そこへの新しいチャットは Yhtye が作業ツリーを作る)。 */
@Composable
private fun OtherBranches(
    names: List<String>,
    statuses: BranchStatuses?,
    open: Boolean,
    onMove: (List<String>) -> Unit,
    onToggle: () -> Unit,
    onNewChat: (String) -> Unit,
) {
    val c = Yhtye.colors
    var menuFor by remember { mutableStateOf<String?>(null) }
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 40.dp)
            .clickable(role = Role.Button, onClick = onToggle)
            .semantics(mergeDescendants = true) {
                contentDescription = "他のブランチ (${names.size})"
                stateDescription = if (open) "開いている" else "閉じている"
            }
            .padding(start = 12.dp, end = 4.dp)
            .testTag("other-branches"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        YIcon(YIcons.ChevronDown, c.tx4, 14.dp, Modifier.rotate(if (open) 0f else -90f))
        YIcon(YIcons.Branch, c.tx3, 14.dp)
        Text("${names.size}", color = c.tx2, fontFamily = MonoFont, fontSize = 12.5.sp)
    }
    if (!open) return
    names.forEach { n ->
        Row(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 8.dp, vertical = 1.dp)
                .clip(ROW_SHAPE)
                .combinedClickable(role = Role.Button, onLongClick = { menuFor = n }) { onNewChat(n) }
                .semantics(mergeDescendants = true) { contentDescription = "$n の新しいチャット" }
                .heightIn(min = 42.dp)
                .padding(start = 30.dp, end = 10.dp)
                .testTag("other-branch-$n"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            StatusIcon(n, statuses)
            YIcon(YIcons.MessagePlus, c.tx3, 16.dp)
            Text(n, color = c.tx2, fontFamily = MonoFont, fontSize = 12.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            ReorderMenu(menuFor == n, { menuFor = null }, names, n, "other-move-$n", onMove)
        }
    }
}

/**
 * 新しいブランチ (PC の NewBranchForm): 名前と開始点。✓ / IME の完了で作り、断られたら理由をフォームに出す。
 * [create] はコアのエラーを投げる。
 */
@Composable
private fun NewBranchForm(overview: GitOverview?, create: suspend (String, String?) -> Unit) {
    val c = Yhtye.colors
    val scope = rememberActionScope()
    var name by rememberSaveable { mutableStateOf("") }
    var from by rememberSaveable { mutableStateOf(HEAD_OPTION) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var pickFrom by remember { mutableStateOf(false) }
    val locals = overview?.branches.orEmpty().map { it.name }.filter { !isInternalBranch(it) }
    val headLabel = "HEAD (${overview?.head ?: "detached"})"
    val submit = {
        if (name.isNotBlank() && !busy) {
            busy = true
            error = null
            scope.launch {
                try {
                    create(name.trim(), from.takeIf { it != HEAD_OPTION })
                } catch (e: RpcException) {
                    error = describe(e)
                    busy = false
                }
            }
        }
    }
    Column(
        Modifier
            .padding(start = 12.dp, end = 12.dp, top = 2.dp, bottom = 10.dp)
            .fillMaxWidth()
            .clip(FORM_SHAPE)
            .border(1.dp, c.lineCard, FORM_SHAPE)
            .background(c.bgInset)
            .padding(10.dp)
            .semantics { contentDescription = "新しいブランチ" }
            .testTag("branch-form"),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        FormField(name, { name = it }, "feature/name", "ブランチ名", enabled = !busy, tag = "branch-name", onDone = submit)
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Box(Modifier.weight(1f)) {
                Row(
                    Modifier
                        .fillMaxWidth()
                        .height(40.dp)
                        .clip(FIELD_SHAPE)
                        .border(1.dp, c.lineCard, FIELD_SHAPE)
                        .background(c.bgCenter)
                        .clickable(enabled = !busy, role = Role.DropdownList) { pickFrom = true }
                        .semantics { contentDescription = "開始点" }
                        .padding(horizontal = 10.dp)
                        .testTag("branch-from"),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        if (from == HEAD_OPTION) headLabel else from,
                        color = c.tx,
                        fontFamily = MonoFont,
                        fontSize = 13.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    YIcon(YIcons.ChevronDown, c.tx4, 14.dp)
                }
                DropdownMenu(expanded = pickFrom, onDismissRequest = { pickFrom = false }) {
                    (listOf(HEAD_OPTION) + locals).forEach { option ->
                        DropdownMenuItem(
                            text = { Text(if (option == HEAD_OPTION) headLabel else option, fontFamily = MonoFont, fontSize = 13.sp) },
                            onClick = {
                                from = option
                                pickFrom = false
                            },
                            modifier = Modifier.testTag("branch-from-${option.ifEmpty { "HEAD" }}"),
                        )
                    }
                }
            }
            val canCreate = !busy && name.isNotBlank()
            Box(
                Modifier
                    .size(40.dp)
                    .clip(CircleShape)
                    .background(c.bgChip)
                    .clickable(enabled = canCreate, role = Role.Button, onClick = submit)
                    .semantics { contentDescription = "作成" }
                    .testTag("branch-create"),
                contentAlignment = Alignment.Center,
            ) { YIcon(if (busy) YIcons.Loader else YIcons.Check, if (canCreate || busy) c.tx else c.tx5, 16.dp, spin = busy) }
        }
        error?.let {
            Text(
                it,
                color = c.handFg,
                fontSize = 12.5.sp,
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite }.testTag("branch-error"),
            )
        }
    }
}

@Composable
private fun FormField(
    value: String,
    onChange: (String) -> Unit,
    placeholder: String,
    label: String,
    enabled: Boolean,
    tag: String,
    onDone: () -> Unit,
) {
    val c = Yhtye.colors
    BasicTextField(
        value = value,
        onValueChange = onChange,
        enabled = enabled,
        singleLine = true,
        textStyle = TextStyle(color = c.tx, fontFamily = MonoFont, fontSize = 13.sp),
        cursorBrush = SolidColor(c.accent),
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false, imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = { onDone() }),
        modifier = Modifier.fillMaxWidth().semantics { contentDescription = label }.testTag(tag),
        decorationBox = { inner ->
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(40.dp)
                    .clip(FIELD_SHAPE)
                    .border(1.dp, c.lineCard, FIELD_SHAPE)
                    .background(c.bgCenter)
                    .padding(horizontal = 10.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                if (value.isEmpty()) Text(placeholder, color = c.tx4, fontFamily = MonoFont, fontSize = 13.sp)
                inner()
            }
        },
    )
}
