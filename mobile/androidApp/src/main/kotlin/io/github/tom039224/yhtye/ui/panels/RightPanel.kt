package io.github.tom039224.yhtye.ui.panels

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.Group
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.State
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.LoadPhase
import io.github.tom039224.yhtye.shared.sync.ProjectView
import io.github.tom039224.yhtye.shared.sync.awaitingFinish
import io.github.tom039224.yhtye.shared.sync.focusGroup
import io.github.tom039224.yhtye.shared.sync.groupLabel
import io.github.tom039224.yhtye.shared.sync.groupProgress
import io.github.tom039224.yhtye.shared.sync.liveSubagents
import io.github.tom039224.yhtye.shared.sync.scopeState
import io.github.tom039224.yhtye.ui.common.EmptyState
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.rememberActionScope
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.conversation.Mention
import io.github.tom039224.yhtye.ui.tasks.CancelTarget
import io.github.tom039224.yhtye.ui.tasks.MetaLine
import io.github.tom039224.yhtye.ui.tasks.StatusChip
import io.github.tom039224.yhtye.ui.tasks.groupChipColors
import io.github.tom039224.yhtye.ui.tasks.groupIcon
import kotlinx.coroutines.launch

// 右パネル (PC の TaskColumn、モックの `#right`): 選択中のチャットが作ったグループだけ。ヘッダ (グループ名 + 動いている
// サブエージェントの数)、扱っているグループ (開いているもの、無ければ最新) の行とタスクのカード、過去のグループは折りたたみ。
// help / checkpoint は状態の表示だけ (扱うのはオーケストレータ)。

private val GCHIP_SHAPE = RoundedCornerShape(7.dp)
private val ALERT_SHAPE = RoundedCornerShape(10.dp)
private val BUTTON_SHAPE = RoundedCornerShape(18.dp)

/** パネルを閉じても残す右パネルの状態 (隠れたパネルは合成しないので)。 */
@Stable
class RightPanelMemory {
    /** 過去のグループを開いている。 */
    var pastOpen by mutableStateOf(false)
}

/**
 * @param onOpenTask カードのタップ (タスク詳細)。
 * @param onMention `@` と右→左のスワイプ (積んだら中央に戻る)。
 * @param onCancel ■ (確認してから中止)。
 */
@Composable
fun RightPanel(
    app: AppState,
    sync: AppStore,
    now: Long,
    memory: RightPanelMemory,
    gestureInsets: WindowInsets,
    onOpenTask: (String) -> Unit,
    onMention: (Mention) -> Unit,
    onCancel: (CancelTarget) -> Unit,
) {
    val c = Yhtye.colors
    val view = app.project
    val state = view?.state?.let { scopeState(it, view.selectedChat) }
    val group = state?.let(::focusGroup)
    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars)) {
        Header(view, state, group)
        HorizontalDivider(color = c.line)
        when {
            view == null -> EmptyState(YIcons.List, "チャットを選ぶとタスクが出ます", testTag = "no-groups")
            view.phase == LoadPhase.Error -> EmptyState(YIcons.Alert, view.loadError ?: "読み込めませんでした", error = true)
            state == null -> EmptyState(YIcons.Loader, "読み込み中…")
            group == null -> EmptyState(
                YIcons.List,
                if (view.selectedChat != null) "グループはまだありません" else "チャットを選ぶとタスクが出ます",
                testTag = "no-groups",
            )
            else -> LazyColumn(
                Modifier.weight(1f).windowInsetsPadding(WindowInsets.navigationBars).testTag("right-panel"),
                contentPadding = PaddingValues(start = 12.dp, end = 10.dp, top = 10.dp, bottom = 24.dp),
            ) {
                val cards = Cards(view, state, now, gestureInsets, onOpenTask, onMention, onCancel)
                groupSection(group, state, sync, cards, onCancel, keyPrefix = "focus")
                val older = state.groups.filter { it.id != group.id }.reversed()
                if (older.isNotEmpty()) {
                    item(key = "past") { PastGroupsToggle(older.size, memory) }
                    if (memory.pastOpen) {
                        older.forEach { g -> groupSection(g, state, sync, cards, onCancel, keyPrefix = "past", topPadding = true) }
                    }
                }
            }
        }
    }
}

/** カードを作るのに要るもの (グループごとに同じ)。 */
private class Cards(
    val view: ProjectView,
    val state: State,
    val now: Long,
    val gestureInsets: WindowInsets,
    val onOpenTask: (String) -> Unit,
    val onMention: (Mention) -> Unit,
    val onCancel: (CancelTarget) -> Unit,
)

private fun LazyListScope.groupSection(
    group: Group,
    state: State,
    sync: AppStore,
    cards: Cards,
    onCancel: (CancelTarget) -> Unit,
    keyPrefix: String,
    topPadding: Boolean = false,
) {
    item(key = "$keyPrefix-bar-${group.id}") { GroupBar(group, state, sync, onCancel, Modifier.padding(top = if (topPadding) 6.dp else 0.dp)) }
    val tasks = state.tasks.filter { it.group == group.id }
    if (tasks.isEmpty()) {
        item(key = "$keyPrefix-empty-${group.id}") { EmptyState(YIcons.List, "タスクはまだありません") }
    }
    items(tasks, key = { "$keyPrefix-task-${it.id}" }) { t ->
        TaskCard(
            task = t,
            state = cards.state,
            view = cards.view,
            now = cards.now,
            gestureInsets = cards.gestureInsets,
            onOpen = { cards.onOpenTask(t.id) },
            onMention = { cards.onMention(Mention(t.id, t.title)) },
            onCancel = { cards.onCancel(CancelTarget.OfTask(t.id)) },
        )
    }
}

@Composable
private fun Header(view: ProjectView?, state: State?, group: Group?) {
    val c = Yhtye.colors
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(start = 16.dp, end = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        YIcon(YIcons.List, c.tx3, 16.dp, Modifier.semantics { contentDescription = "tasks" })
        Box(Modifier.weight(1f)) {
            if (group != null) {
                Text(
                    group.title,
                    color = c.tx,
                    fontSize = 13.5.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier
                        .clip(GCHIP_SHAPE)
                        .background(c.bgChip)
                        .padding(horizontal = 10.dp, vertical = 4.dp)
                        .testTag("group-title"),
                )
            }
        }
        val live = if (view != null && state != null) liveSubagents(view, state) else 0
        Row(
            Modifier.semantics(mergeDescendants = true) { contentDescription = "動いているサブエージェントのセッション数 $live" },
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            YIcon(YIcons.Bot, c.tx3, 14.dp)
            Text("$live", color = c.tx3, fontFamily = MonoFont, fontSize = 12.sp, modifier = Modifier.testTag("live-subagents"))
        }
    }
}

/** グループの状態・ID・ブランチ・完了数と操作 (中止、マージの再試行)。 */
@Composable
private fun GroupBar(group: Group, state: State, sync: AppStore, onCancel: (CancelTarget) -> Unit, modifier: Modifier = Modifier) {
    val c = Yhtye.colors
    val scope = rememberActionScope()
    val p = groupProgress(state, group.id)
    val cancellable = group.status == GroupStatus.Active || group.status == GroupStatus.MergeBlocked
    Column(modifier.fillMaxWidth().semantics { contentDescription = "group ${group.id}" }.testTag("group-${group.id}")) {
        Row(Modifier.fillMaxWidth().heightIn(min = 40.dp).padding(start = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            StatusChip(
                groupIcon(group.status, awaitingFinish(state, group)),
                groupLabel(state, group),
                groupChipColors(group.status),
                labelTag = "group-status-${group.id}",
            )
            Text(group.id, color = c.tx2, fontFamily = MonoFont, fontSize = 11.5.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(start = 8.dp))
            Spacer(Modifier.weight(1f))
            Row(
                Modifier.semantics(mergeDescendants = true) { contentDescription = "${p.done}/${p.total} 完了" },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                YIcon(YIcons.Check, c.tx3, 12.dp)
                Text("${p.done}/${p.total}", color = c.tx3, fontFamily = MonoFont, fontSize = 11.5.sp, modifier = Modifier.testTag("group-count-${group.id}"))
            }
            if (p.handling > 0) {
                Row(
                    Modifier.padding(start = 8.dp).semantics(mergeDescendants = true) { contentDescription = "${p.handling} 件 対処中" },
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(3.dp),
                ) {
                    YIcon(YIcons.Alert, c.handBar, 12.dp)
                    Text("${p.handling}", color = c.handBar, fontFamily = MonoFont, fontSize = 11.5.sp)
                }
            }
            if (cancellable) {
                IconButton(
                    onClick = { onCancel(CancelTarget.OfGroup(group.id)) },
                    modifier = Modifier.size(40.dp).semantics { contentDescription = "グループを中止" }.testTag("cancel-group-${group.id}"),
                ) { YIcon(YIcons.Stop, c.tx3, 12.dp) }
            }
        }
        Row(
            Modifier.fillMaxWidth().padding(start = 2.dp, bottom = 10.dp).semantics { contentDescription = "グループブランチ ← マージ先 (base)" },
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            YIcon(YIcons.Branch, c.tx4, 12.dp)
            Text(group.groupBranch, color = c.tx4, fontFamily = MonoFont, fontSize = 11.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
            YIcon(YIcons.ArrowLeft, c.tx4, 12.dp)
            Text(group.baseBranch, color = c.tx4, fontFamily = MonoFont, fontSize = 11.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
        }
        if (group.status == GroupStatus.MergeBlocked) {
            MergeBlocked(group) { scope.launch { sync.retryGroupMerge(group.id) } }
        }
        val detail = group.detail
        if (detail != null && group.status != GroupStatus.MergeBlocked) {
            MetaLine(null, detail, c.tx4, Modifier.padding(start = 2.dp, end = 2.dp, bottom = 10.dp))
        }
    }
}

/** マージが保留されている理由と「マージを再試行」(PC の group-alert)。 */
@Composable
private fun MergeBlocked(group: Group, onRetry: () -> Unit) {
    val c = Yhtye.colors
    Column(
        Modifier
            .padding(bottom = 10.dp)
            .fillMaxWidth()
            .clip(ALERT_SHAPE)
            .background(c.handCardBg)
            .border(1.dp, c.handCardLine, ALERT_SHAPE)
            .padding(horizontal = 12.dp, vertical = 9.dp)
            .semantics { liveRegion = LiveRegionMode.Polite },
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            YIcon(YIcons.Alert, c.handBar, 16.dp, Modifier.padding(top = 2.dp))
            Text(
                "base ブランチへのマージが保留されています: ${group.detail ?: "理由不明"}",
                color = c.handCardTx,
                fontSize = 13.sp,
                lineHeight = 20.sp,
                modifier = Modifier.weight(1f).testTag("merge-blocked-${group.id}"),
            )
        }
        Row(
            Modifier
                .align(Alignment.End)
                .height(36.dp)
                .clip(BUTTON_SHAPE)
                .background(c.bgChip)
                .clickable(role = Role.Button, onClick = onRetry)
                .padding(horizontal = 14.dp)
                .testTag("retry-merge-${group.id}"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            YIcon(YIcons.Refresh, c.tx, 14.dp)
            Text("マージを再試行", color = c.tx, fontSize = 13.5.sp, fontWeight = FontWeight.SemiBold)
        }
    }
}

@Composable
private fun PastGroupsToggle(count: Int, memory: RightPanelMemory) {
    val c = Yhtye.colors
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 40.dp)
            .clickable(role = Role.Button, onClick = { memory.pastOpen = !memory.pastOpen })
            .semantics(mergeDescendants = true) { contentDescription = "過去のグループ ($count)" }
            .padding(horizontal = 4.dp)
            .testTag("past-groups"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        YIcon(YIcons.ChevronDown, c.tx3, 14.dp, Modifier.rotate(if (memory.pastOpen) 0f else -90f))
        YIcon(YIcons.History, c.tx3, 14.dp)
        Text("過去のグループ $count", color = c.tx3, fontSize = 12.sp, fontWeight = FontWeight.Medium)
    }
}
