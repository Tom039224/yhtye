package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.HelpKind
import io.github.tom039224.yhtye.shared.api.generated.StepKind
import io.github.tom039224.yhtye.shared.api.generated.Task
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus

// 状態の表示名 (orchestration-model.md §4、日本語は orchestrator-desktop.md §3.5)。PC の `src/ui/labels.ts` の移植。

/** 設計の状態の系統。それぞれにトークンの色がある。 */
enum class Tone { Waiting, Implementing, Reviewing, Handling, Done, Cancelled }

fun taskTone(task: Task): Tone = when (task.status) {
    TaskStatus.Pending, TaskStatus.AwaitingInstruction -> Tone.Waiting
    TaskStatus.Running ->
        if (task.steps.getOrNull(task.current.toInt())?.kind == StepKind.Review) Tone.Reviewing else Tone.Implementing
    TaskStatus.Checkpoint, TaskStatus.Handling, TaskStatus.Interrupted -> Tone.Handling
    TaskStatus.Merging, TaskStatus.Done -> Tone.Done
    TaskStatus.Cancelled -> Tone.Cancelled
    // 新しい PC の知らない状態: 待機として出す
    TaskStatus.Unrecognized -> Tone.Waiting
}

val TONE_LABEL: Map<Tone, String> = mapOf(
    Tone.Waiting to "待機",
    Tone.Implementing to "実装中",
    Tone.Reviewing to "レビュー中",
    Tone.Handling to "対処中",
    Tone.Done to "完了",
    Tone.Cancelled to "中止",
)

fun groupStatusLabel(status: GroupStatus): String = when (status) {
    GroupStatus.Active -> "進行中"
    GroupStatus.Finishing -> "マージ中"
    GroupStatus.Done -> "完了"
    GroupStatus.Cancelled -> "中止"
    GroupStatus.MergeBlocked -> "マージ保留"
    GroupStatus.Unrecognized -> "不明"
}

fun helpKindLabel(kind: HelpKind): String = when (kind) {
    HelpKind.Blocked -> "詰まり"
    HelpKind.Question -> "質問"
    HelpKind.Policy -> "方針の相談"
    HelpKind.MergeConflict -> "マージコンフリクト"
    HelpKind.ReviewRoundsExhausted -> "レビュー回数超過"
    HelpKind.ProtocolViolation -> "報告なしでターン終了"
    HelpKind.AgentStopped -> "エージェント停止"
    HelpKind.AgentCrashed -> "エージェント異常終了"
    HelpKind.DirtyReadonlyTree -> "読み取り専用の作業ツリーが変更された"
    HelpKind.GitFailed -> "git 操作の失敗"
    HelpKind.Unrecognized -> "不明"
}

fun isTerminal(task: Task): Boolean = task.status == TaskStatus.Done || task.status == TaskStatus.Cancelled
