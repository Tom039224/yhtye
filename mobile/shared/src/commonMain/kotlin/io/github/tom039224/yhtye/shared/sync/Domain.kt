package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.Counters
import io.github.tom039224.yhtye.shared.api.generated.DomainConfig
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.Group
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.Help
import io.github.tom039224.yhtye.shared.api.generated.HelpState
import io.github.tom039224.yhtye.shared.api.generated.State
import io.github.tom039224.yhtye.shared.api.generated.Step
import io.github.tom039224.yhtye.shared.api.generated.StepSpec
import io.github.tom039224.yhtye.shared.api.generated.StepStatus
import io.github.tom039224.yhtye.shared.api.generated.Task
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus

// ドメインの reducer: コアの `State::apply` (crates/yhtye-core/src/domain/event.rs) と PC の `src/store/domain.ts` の移植。
// 純関数で、入力を書き換えない。Rust の reducer と同じ結果になることは記録したイベント列で確かめる (DomainTest)。

private fun stepFromSpec(spec: StepSpec): Step = Step(
    kind = spec.kind,
    instruction = spec.instruction,
    status = StepStatus.Pending,
    result = null,
    verdict = null,
    note = null,
    nudges = 0,
)

private inline fun State.mapGroup(id: String, f: (Group) -> Group): State =
    copy(groups = groups.map { if (it.id == id) f(it) else it })

private inline fun State.mapHelp(id: String, f: (Help) -> Help): State =
    copy(helps = helps.map { if (it.id == id) f(it) else it })

private inline fun State.mapTask(id: String, f: (Task) -> Task): State =
    copy(tasks = tasks.map { if (it.id == id) f(it) else it })

private inline fun State.mapStep(task: String, index: Long, crossinline f: (Step) -> Step): State =
    mapTask(task) { t ->
        if (index < t.steps.size) t.copy(steps = t.steps.mapIndexed { i, s -> if (i.toLong() == index) f(s) else s }) else t
    }

/** チャットと、そのグループ・タスク・help・受信箱を落とす (`State::remove_chat`)。カウンタは戻さない。 */
private fun State.removeChat(chat: String): State {
    val groupIds = groups.filter { it.chat == chat }.mapTo(HashSet()) { it.id }
    val taskIds = tasks.filter { it.group in groupIds }.mapTo(HashSet()) { it.id }
    return copy(
        chats = chats.filter { it.id != chat },
        groups = groups.filter { it.chat != chat },
        tasks = tasks.filter { it.group !in groupIds },
        helps = helps.filter { it.task !in taskIds },
        inbox = inbox.filter { it.chat != chat },
    )
}

/** イベントを 1 つ適用する。Rust の reducer と同じく、知らない ID では何も変えない。 */
fun applyDomainEvent(state: State, event: DomainEvent): State = when (event) {
    is DomainEvent.GroupCreated ->
        state.copy(counters = state.counters.copy(groups = state.counters.groups + 1), groups = state.groups + event.group)
    // `finish_group` の `into` (Stage 8e): グループは別のブランチへマージする
    is DomainEvent.GroupBaseChanged -> state.mapGroup(event.group) { it.copy(baseBranch = event.to) }
    is DomainEvent.GroupFinishing ->
        state.mapGroup(event.group) { it.copy(status = GroupStatus.Finishing, finishSummary = event.summary) }
    is DomainEvent.GroupFinishReminded -> state.mapGroup(event.group) { it.copy(finishNudges = it.finishNudges + 1) }
    is DomainEvent.GroupMergeFinished -> state.mapGroup(event.group) {
        it.copy(status = if (event.ok) GroupStatus.Done else GroupStatus.MergeBlocked, detail = event.detail)
    }
    is DomainEvent.GroupCancelled ->
        state.mapGroup(event.group) { it.copy(status = GroupStatus.Cancelled, detail = event.reason) }
    is DomainEvent.HelpRaised ->
        state.copy(counters = state.counters.copy(helps = state.counters.helps + 1), helps = state.helps + event.help)
    is DomainEvent.HelpAnswered -> {
        val help = state.helps.find { it.id == event.help }
        if (help == null) {
            state
        } else {
            // 答えは新しいプロンプト: エージェントの催促の回数を戻す
            state.mapHelp(event.help) { it.copy(state = HelpState.Answered, reply = event.reply) }
                .mapStep(help.task, help.step) { it.copy(nudges = 0) }
        }
    }
    is DomainEvent.HelpClosed -> state.mapHelp(event.help) { it.copy(state = HelpState.Closed) }
    is DomainEvent.HelpAgentLost -> state.mapHelp(event.help) { it.copy(agentLost = true) }
    is DomainEvent.InboxQueued -> state.copy(
        counters = state.counters.copy(inbox = maxOf(state.counters.inbox, event.entry.id)),
        inbox = state.inbox + event.entry,
    )
    is DomainEvent.InboxDelivered ->
        state.copy(inbox = state.inbox.filter { it.chat != event.chat || it.id > event.upTo })
    is DomainEvent.ChatCreated ->
        state.copy(counters = state.counters.copy(chats = state.counters.chats + 1), chats = state.chats + event.chat)
    is DomainEvent.ChatTitled ->
        state.copy(chats = state.chats.map { if (it.id == event.chat) it.copy(title = event.title) else it })
    is DomainEvent.ChatDeleted -> state.removeChat(event.chat)
    is DomainEvent.TaskCreated ->
        state.copy(counters = state.counters.copy(tasks = state.counters.tasks + 1), tasks = state.tasks + event.task)
    is DomainEvent.InstructionSet -> state.mapTask(event.task) { it.copy(instruction = event.instruction) }
    is DomainEvent.TaskStatusChanged -> state.mapTask(event.task) { it.copy(status = event.status) }
    is DomainEvent.TaskCancelled ->
        state.mapTask(event.task) { it.copy(status = TaskStatus.Cancelled, cancelReason = event.reason) }
    is DomainEvent.WorkspaceReady -> state.mapTask(event.task) { it.copy(workdir = event.path) }
    is DomainEvent.StepsReplaced -> state.mapTask(event.task) { t ->
        t.copy(steps = t.steps.take(event.from.toInt().coerceAtMost(t.steps.size)) + event.steps.map(::stepFromSpec))
    }
    is DomainEvent.ReviewStepsInserted -> state.mapTask(event.task) { t ->
        val at = minOf(event.after + 1, t.steps.size.toLong()).toInt()
        t.copy(
            steps = t.steps.take(at) + event.steps.map(::stepFromSpec) + t.steps.drop(at),
            reviewRounds = t.reviewRounds + 1,
        )
    }
    is DomainEvent.StepStarted -> state.mapTask(event.task) { t ->
        t.copy(
            current = event.step,
            steps = t.steps.mapIndexed { i, s ->
                if (i.toLong() == event.step) s.copy(status = StepStatus.Running, note = event.note, nudges = 0) else s
            },
        )
    }
    is DomainEvent.StepCompleted -> state.mapStep(event.task, event.step) {
        it.copy(status = StepStatus.Done, result = event.result, verdict = event.verdict)
    }
    is DomainEvent.StepReset -> state.mapStep(event.task, event.step) { it.copy(status = StepStatus.Pending) }
    is DomainEvent.NudgeSent -> state.mapStep(event.task, event.step) { it.copy(nudges = it.nudges + 1) }
    // ログ用の事実だけ (Rust の reducer も状態を変えない)
    is DomainEvent.TaskMessageSent, is DomainEvent.TaskMessageResolved -> state
    // 新しい PC が出した知らないイベント: 状態は変えない
    is DomainEvent.Unrecognized -> state
}

/** [project] の空の状態 (コアが新しいプロジェクトを始める状態)。 */
fun emptyState(project: String, config: DomainConfig): State = State(
    project = project,
    config = config,
    chats = emptyList(),
    groups = emptyList(),
    tasks = emptyList(),
    helps = emptyList(),
    inbox = emptyList(),
    counters = Counters(chats = 0, groups = 0, tasks = 0, helps = 0, inbox = 0),
)
