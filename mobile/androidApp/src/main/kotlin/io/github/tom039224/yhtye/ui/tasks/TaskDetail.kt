package io.github.tom039224.yhtye.ui.tasks

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.api.generated.Task
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus
import io.github.tom039224.yhtye.shared.api.generated.Verdict
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.ProjectView
import io.github.tom039224.yhtye.shared.sync.formatElapsed
import io.github.tom039224.yhtye.shared.sync.isTerminal
import io.github.tom039224.yhtye.shared.sync.openHelps
import io.github.tom039224.yhtye.shared.sync.taskAgent
import io.github.tom039224.yhtye.shared.sync.taskElapsedMs
import io.github.tom039224.yhtye.shared.sync.taskSessions
import io.github.tom039224.yhtye.shared.sync.taskTone
import io.github.tom039224.yhtye.ui.common.EmptyState
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.rememberActionScope
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.common.breatheAlpha
import io.github.tom039224.yhtye.ui.conversation.Mention
import io.github.tom039224.yhtye.ui.conversation.TranscriptList
import io.github.tom039224.yhtye.ui.conversation.TranscriptSource
import kotlinx.coroutines.launch

// タスク詳細 (PC の AgentOutput / BottomPanel のタスクのタブ、モックの `#detail`): タスクの ID・題・状態・担当・経過、
// Step の並び、対処中の help の文、エージェントの出力 (中央の会話と同じ部品。セッションが複数あればタブで切り替え)。
// メニューは PC にある操作だけ (`@` の引用、中止)。

private val TAB_SHAPE = RoundedCornerShape(17.dp)

/**
 * [taskId] の詳細。タスクが無くなったら (チャットの削除) [onBack] で閉じる。
 *
 * @param onMention `@` でオーケストレータへ引用する (積んだら中央に戻る)。
 * @param onCancel ■ (確認してからタスクを中止)。
 */
@Composable
fun TaskDetail(
    app: AppState,
    sync: AppStore,
    taskId: String,
    now: Long,
    onBack: () -> Unit,
    onMention: (Mention) -> Unit,
    onCancel: (CancelTarget) -> Unit,
) {
    val view = app.project
    val task = view?.state?.tasks?.find { it.id == taskId }
    if (view == null || task == null) {
        LaunchedEffect(Unit) { onBack() }
        return
    }
    val scope = rememberActionScope()
    val sessions = taskSessions(view, task.id)
    var chosen by rememberSaveable(task.id) { mutableStateOf<String?>(null) }
    val session = chosen?.takeIf { it in sessions } ?: sessions.lastOrNull()
    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars).testTag("task-detail")) {
        DetailHeader(task, onBack, onMention, onCancel)
        HorizontalDivider(color = Yhtye.colors.line)
        Summary(view, task, now)
        if (sessions.isNotEmpty()) {
            SessionTabs(view, sessions, session, onChoose = { chosen = it })
            HorizontalDivider(color = Yhtye.colors.line)
        }
        val results: (@Composable () -> Unit)? = if (task.steps.any { it.result != null }) ({ StepResults(task) }) else null
        Box(Modifier.weight(1f).fillMaxWidth().windowInsetsPadding(WindowInsets.navigationBars)) {
            if (session == null) {
                Column(Modifier.fillMaxSize()) {
                    results?.invoke()
                    EmptyState(YIcons.Bot, "${task.id} のエージェントは未起動です")
                }
            } else {
                key(session) {
                    TranscriptList(
                        TranscriptSource(view, view.transcripts[session].orEmpty(), view.streaming[session], state = view.state),
                        onLoadOlder = { scope.launch { sync.loadOlderHistory() } },
                        modifier = Modifier.fillMaxSize(),
                        lead = results,
                    )
                }
            }
        }
    }
}

@Composable
private fun DetailHeader(task: Task, onBack: () -> Unit, onMention: (Mention) -> Unit, onCancel: (CancelTarget) -> Unit) {
    val c = Yhtye.colors
    val tone = taskTone(task)
    Row(Modifier.fillMaxWidth().heightIn(min = 60.dp).padding(start = 4.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        IconButton(onClick = onBack, modifier = Modifier.semantics { contentDescription = "戻る" }.testTag("detail-back")) {
            YIcon(YIcons.Back, c.tx2, 20.dp)
        }
        Column(Modifier.weight(1f).padding(vertical = 8.dp, horizontal = 2.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TaskStatusChip(task, tone)
                Text("${task.id} · ${task.group}", color = c.tx4, fontFamily = MonoFont, fontSize = 11.5.sp, maxLines = 1)
            }
            Text(
                task.title,
                color = c.tx,
                fontSize = 15.5.sp,
                fontWeight = FontWeight.SemiBold,
                lineHeight = 21.sp,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(top = 4.dp).testTag("detail-title"),
            )
        }
        DetailMenu(task, onMention, onCancel)
    }
}

/** PC のタスクカードと同じ操作だけ: オーケストレータへ引用、中止 (終わったタスクには無い)。 */
@Composable
private fun DetailMenu(task: Task, onMention: (Mention) -> Unit, onCancel: (CancelTarget) -> Unit) {
    val c = Yhtye.colors
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }, modifier = Modifier.semantics { contentDescription = "${task.id} の操作" }.testTag("detail-menu")) {
            YIcon(YIcons.More, c.tx2, 20.dp)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text("オーケストレータへ引用") },
                leadingIcon = { YIcon(YIcons.At, c.tx3, 18.dp) },
                onClick = {
                    open = false
                    onMention(Mention(task.id, task.title))
                },
                modifier = Modifier.testTag("detail-mention"),
            )
            if (!isTerminal(task)) {
                DropdownMenuItem(
                    text = { Text("エージェントを停止 (タスクを中止)") },
                    leadingIcon = { YIcon(YIcons.Stop, c.handBar, 14.dp) },
                    onClick = {
                        open = false
                        onCancel(CancelTarget.OfTask(task.id))
                    },
                    modifier = Modifier.testTag("detail-cancel"),
                )
            }
        }
    }
}

/** Step の並びと担当・経過、中断の注記、対処中の help の文 (状態の表示だけ)、中止の理由。 */
@Composable
private fun Summary(view: ProjectView, task: Task, now: Long) {
    val c = Yhtye.colors
    val tone = taskTone(task)
    val agent = taskAgent(view, task)
    val elapsed = taskElapsedMs(view, task, now)?.let(::formatElapsed)
    val helps = view.state?.let { openHelps(it, task.id) }.orEmpty()
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            StepsRow(task, tone, Modifier.weight(1f))
            if (agent != null) AgentLabel(agent, elapsed, c.tx4)
        }
        if (task.status == TaskStatus.Interrupted) {
            Row(horizontalArrangement = Arrangement.spacedBy(7.dp)) {
                YIcon(YIcons.Refresh, c.tx3, 14.dp, Modifier.padding(top = 3.dp))
                Text(INTERRUPTED_NOTE, color = c.tx2, fontSize = 13.5.sp, lineHeight = 21.sp)
            }
        }
        helps.forEach { HelpNote(it, c.handCardTx, c.handBar) }
        task.cancelReason?.let { MetaLine(YIcons.Ban, it, c.tx4, description = "中止した理由: $it") }
    }
    HorizontalDivider(color = c.line)
}

/** セッションのタブ (implementer / review-n)。はみ出したら横にスクロールする (その上の横スワイプは画面を閉じない)。 */
@Composable
private fun SessionTabs(view: ProjectView, sessions: List<String>, chosen: String?, onChoose: (String) -> Unit) {
    val c = Yhtye.colors
    val scroll = rememberScrollState()
    Row(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(scroll, enabled = scroll.maxValue > 0)
            .padding(horizontal = 12.dp, vertical = 8.dp)
            .testTag("session-tabs"),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        sessions.forEach { key ->
            val selected = key == chosen
            val record = view.sessions.find { it.sessionKey == key }
            val state = when {
                record == null -> null
                record.turnRunning -> "working"
                else -> record.status.wire
            }
            Row(
                Modifier
                    .height(34.dp)
                    .clip(TAB_SHAPE)
                    .then(if (selected) Modifier.background(c.bgChip) else Modifier.border(1.dp, c.lineCard, TAB_SHAPE))
                    .clickable(role = Role.Tab, onClick = { onChoose(key) })
                    .semantics {
                        this.selected = selected
                        contentDescription = if (state != null) "$key · $state" else key
                    }
                    .padding(horizontal = 14.dp)
                    .testTag("session-tab-$key"),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(7.dp),
            ) {
                SessionDot(record?.let { if (it.turnRunning) null else it.status }, working = record?.turnRunning == true)
                Text(
                    key.substringAfter('/', key),
                    color = if (selected) c.tx else c.tx3,
                    fontFamily = MonoFont,
                    fontSize = 12.5.sp,
                    fontWeight = FontWeight.Medium,
                )
            }
        }
    }
}

/** セッションの状態の点 (PC の status-dot: 作業中 = 青、待機 = 緑、終わった・未起動 = 中を抜いた丸)。 */
@Composable
private fun SessionDot(status: SessionStatus?, working: Boolean) {
    val c = Yhtye.colors
    when {
        working -> Box(Modifier.size(8.dp).alpha(breatheAlpha(periodMs = 2_000)).background(c.implBar, CircleShape))
        status == SessionStatus.Live -> Box(Modifier.size(8.dp).background(c.doneBar, CircleShape))
        else -> Box(Modifier.size(8.dp).border(1.5.dp, c.tx4, CircleShape))
    }
}

/** 終わった Step の結果 (レビューは判定のアイコン付き)。 */
@Composable
private fun StepResults(task: Task) {
    val c = Yhtye.colors
    Column(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 0.dp, bottom = 4.dp).semantics { contentDescription = "step results" },
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        task.steps.filter { it.result != null }.forEach { s ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.testTag("step-result")) {
                YIcon(stepIcon(s.kind, task.kind), c.tx4, 14.dp, Modifier.padding(top = 2.dp))
                s.verdict?.let { v ->
                    val ok = v == Verdict.Approve
                    YIcon(if (ok) YIcons.Check else YIcons.Refresh, if (ok) c.doneBar else c.handBar, 12.dp, Modifier.padding(top = 3.dp))
                }
                Text(s.result.orEmpty(), color = c.tx3, fontFamily = MonoFont, fontSize = 12.sp, lineHeight = 18.sp, modifier = Modifier.weight(1f))
            }
        }
        Spacer(Modifier.height(4.dp))
    }
}
