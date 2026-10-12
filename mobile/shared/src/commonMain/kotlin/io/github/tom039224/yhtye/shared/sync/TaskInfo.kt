package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.Group
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.Help
import io.github.tom039224.yhtye.shared.api.generated.HelpState
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.api.generated.State
import io.github.tom039224.yhtye.shared.api.generated.StepStatus
import io.github.tom039224.yhtye.shared.api.generated.Task
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus
import io.github.tom039224.yhtye.shared.api.generated.TextKind

// タスク列 (右パネル) がタスクについて出すもの。ドメインの状態とタスクのエージェントの会話から導く。
// PC の `src/ui/taskInfo.ts` の移植 (アイコンは名前の enum)。

/** タスクのセッションキー (`T-1/implementer`, `T-1/review-2`, ...)。起動した順。 */
fun taskSessions(view: ProjectView, task: String): List<String> {
    val prefix = "$task/"
    val keys = LinkedHashSet<String>()
    view.transcripts.keys.filterTo(keys) { it.startsWith(prefix) }
    view.sessions.map { it.sessionKey }.filterTo(keys) { it.startsWith(prefix) }
    val firstSeq = { k: String -> view.transcripts[k]?.firstOrNull()?.seq ?: Long.MAX_VALUE }
    return keys.sortedBy(firstSeq)
}

/** タスク列が扱うグループ: 開いているもの、無ければ最新。 */
fun focusGroup(state: State): Group? = state.groups.lastOrNull(::isOpenGroup) ?: state.groups.lastOrNull()

fun isOpenGroup(g: Group): Boolean = g.status == GroupStatus.Active || g.status == GroupStatus.Finishing

/**
 * 開いているグループの全タスクが落ち着いた (終端か、もう始まれない) が、オーケストレータがまだグループを
 * 終えていない: `finish_group` を呼ぶまで base ブランチへはマージされない (orchestration-model.md §3)。
 */
fun awaitingFinish(state: State, group: Group): Boolean {
    if (group.status != GroupStatus.Active) return false
    val tasks = state.tasks.filter { it.group == group.id }
    return tasks.isNotEmpty() && tasks.all { isTerminal(it) || isBlocked(state, it) }
}

/** 依存先が中止された保留中のタスク (もう始まれない)。 */
private fun isBlocked(state: State, task: Task, depth: Int = 0): Boolean {
    if (task.status != TaskStatus.Pending || depth > state.tasks.size) return false
    return task.dependsOn.any { id ->
        val dep = state.tasks.find { it.id == id }
        dep != null && (dep.status == TaskStatus.Cancelled || isBlocked(state, dep, depth + 1))
    }
}

/** グループの状態の表示 (「全タスク完了・未マージ」を区別する)。 */
fun groupLabel(state: State, group: Group): String =
    if (awaitingFinish(state, group)) "完了待ち" else groupStatusLabel(group.status)

data class GroupProgress(val done: Int, val total: Int, val handling: Int)

fun groupProgress(state: State, group: String): GroupProgress {
    val tasks = state.tasks.filter { it.group == group && it.status != TaskStatus.Cancelled }
    return GroupProgress(
        done = tasks.count { it.status == TaskStatus.Done },
        total = tasks.size,
        handling = tasks.count { taskTone(it) == Tone.Handling },
    )
}

/** タスクのステップのうち終わった割合 (実行中のステップは半分)。 */
fun taskProgress(task: Task): Double {
    if (task.steps.isEmpty()) return 0.0
    val done = task.steps.count { it.status == StepStatus.Done }
    val running = if (task.steps.any { it.status == StepStatus.Running }) 0.5 else 0.0
    return minOf(1.0, (done + running) / task.steps.size)
}

/** 動いている系統は進捗のバーと点滅する点を出す (設計 §3.5)。 */
fun isActiveTone(tone: Tone): Boolean = tone == Tone.Implementing || tone == Tone.Reviewing

private const val FIRST_LINE_CHARS = 90

/** [text] の 1 行目 (長ければ [max] 文字で切って `…`)。 */
fun firstLine(text: String, max: Int = FIRST_LINE_CHARS): String {
    val line = text.trim().substringBefore('\n')
    return if (line.length > max) "${line.take(max)}…" else line
}

/** カードの活動ログの 1 行: 何が起きて、どうなったか。 */
data class LogLine(
    val kind: Kind,
    val text: String,
    /** ツール呼び出しの結果 (`completed`, `failed`, `in_progress`...)。 */
    val status: String? = null,
) {
    enum class Kind { Message, Tool, Error }
}

private fun logLine(item: TranscriptItem): LogLine? = when (item) {
    is TranscriptItem.Tool -> {
        val calls = if (item.calls.isNotEmpty()) " → ${item.calls.joinToString(", ")}" else ""
        LogLine(LogLine.Kind.Tool, "${item.title}$calls", item.status)
    }
    is TranscriptItem.YhtyeTool -> LogLine(LogLine.Kind.Tool, item.tool, if (item.ok) "completed" else "failed")
    is TranscriptItem.Text -> if (item.textKind == TextKind.Message) LogLine(LogLine.Kind.Message, firstLine(item.text)) else null
    is TranscriptItem.Lifecycle -> if (item.error) LogLine(LogLine.Kind.Error, item.text) else null
    is TranscriptItem.Turn -> if (item.error) LogLine(LogLine.Kind.Error, "turn ended: ${item.outcome}") else null
    else -> null
}

/** タスクの最新のセッションの最近の活動 (多くて [n] 行、古い順)。 */
fun taskLog(view: ProjectView, task: String, n: Int = 2): List<LogLine> {
    val key = taskSessions(view, task).lastOrNull() ?: return emptyList()
    val lines = mutableListOf<LogLine>()
    view.streaming[key]?.message?.takeIf { it.isNotEmpty() }?.let { lines += LogLine(LogLine.Kind.Message, firstLine(it)) }
    val items = view.transcripts[key].orEmpty()
    for (i in items.indices.reversed()) {
        if (lines.size >= n) break
        logLine(items[i])?.let { lines += it }
    }
    return lines.take(n).reversed()
}

/** タスクを今 (か最後に) 扱っているセッション。例: `implementer`。 */
fun taskAgent(view: ProjectView, task: Task): String? {
    val key = taskSessions(view, task.id).lastOrNull() ?: return null
    return key.split("/").getOrNull(1) ?: key
}

/** タスクに掛かった時間 (最初から最後のイベントまで。終わっていなければ [now] まで)。 */
fun taskElapsedMs(view: ProjectView, task: Task, now: Long): Long? {
    val items = taskSessions(view, task.id).flatMap { view.transcripts[it].orEmpty() }
    if (items.isEmpty()) return null
    val start = items.minOf { it.ts }
    val end = if (isTerminal(task)) items.maxOf { it.ts } else now
    return maxOf(0L, end - start)
}

fun formatElapsed(ms: Long): String {
    val minutes = ms / 60_000
    return when {
        minutes < 1 -> "<1m"
        minutes < 60 -> "${minutes}m"
        else -> "${minutes / 60}h ${minutes % 60}m"
    }
}

/** タスクの開いている help。 */
fun openHelps(state: State, task: String): List<Help> = state.helps.filter { it.task == task && it.state == HelpState.Open }

/** カードの下の灰色の行: アイコン、短い文、全文 (PC ではツールチップ)。 */
data class TaskMeta(val icon: MetaIcon, val text: String, val title: String? = null)

enum class MetaIcon { GitMerge, Hourglass }

/** 設計の灰色の行 (無ければ null)。 */
fun taskMeta(state: State, task: Task): TaskMeta? {
    val group = state.groups.find { it.id == task.group }
    if (task.status == TaskStatus.Done && group != null && isOpenGroup(group)) {
        if (group.status == GroupStatus.Finishing) return TaskMeta(MetaIcon.GitMerge, "マージ中", "グループを base ブランチへマージ中")
        if (awaitingFinish(state, group)) {
            return TaskMeta(MetaIcon.Hourglass, "グループの完了待ち", "全タスク完了 — オーケストレータのグループ完了 (マージ) 待ち")
        }
        return TaskMeta(MetaIcon.Hourglass, "他タスクの完了待ち", "同グループの他タスク完了を待機 — 送信保留")
    }
    if (openHelps(state, task.id).isNotEmpty()) {
        return TaskMeta(MetaIcon.Hourglass, "対処待ち", "エージェントは待機中 · オーケストレータが対処中")
    }
    if (task.status == TaskStatus.AwaitingInstruction) return TaskMeta(MetaIcon.Hourglass, "指示待ち", "オーケストレータの指示待ち")
    if (task.status == TaskStatus.Pending && task.dependsOn.isNotEmpty()) {
        val deps = task.dependsOn.joinToString(", ")
        return TaskMeta(MetaIcon.Hourglass, deps, "$deps の完了待ち")
    }
    return null
}

/** [state] のタスクの動いているサブエージェントの数 (ヘッダの `subagents N`)。 */
fun liveSubagents(view: ProjectView, state: State): Int {
    val tasks = state.tasks.mapTo(HashSet()) { it.id }
    return view.sessions.count { it.status == SessionStatus.Live && it.sessionKey.substringBefore('/') in tasks }
}
