package io.github.tom039224.yhtye.ui.tasks

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.Help
import io.github.tom039224.yhtye.shared.api.generated.HelpKind
import io.github.tom039224.yhtye.shared.api.generated.StepKind
import io.github.tom039224.yhtye.shared.api.generated.StepStatus
import io.github.tom039224.yhtye.shared.api.generated.Task
import io.github.tom039224.yhtye.shared.api.generated.TaskKind
import io.github.tom039224.yhtye.shared.api.generated.Verdict
import io.github.tom039224.yhtye.shared.sync.MetaIcon
import io.github.tom039224.yhtye.shared.sync.TONE_LABEL
import io.github.tom039224.yhtye.shared.sync.Tone
import io.github.tom039224.yhtye.shared.sync.helpKindLabel
import io.github.tom039224.yhtye.shared.sync.isActiveTone
import io.github.tom039224.yhtye.shared.sync.isTerminal
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.common.breatheAlpha
import io.github.tom039224.yhtye.ui.conversation.toneIcon

// タスクとグループの表示の部品 (PC の `src/ui/statusIcons.tsx` とモックの `.chip` / `.steps` / `.note`)。右パネルの
// カードとタスク詳細が使う。言葉は PC と同じ (PC ではツールチップ、スマホでは見せる)。

private val CHIP_SHAPE = RoundedCornerShape(6.dp)

/** チップの文字を上下の中央に置く (和文の書体は余白が下に寄るため、行の高さを詰めて中央に揃える)。 */
private val CHIP_TEXT = TextStyle(
    lineHeight = 13.sp,
    platformStyle = PlatformTextStyle(includeFontPadding = false),
    lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.Both),
)

/** Step の種類の言葉 (PC の `STEP_LABEL`)。 */
fun stepLabel(kind: StepKind): String = when (kind) {
    StepKind.Implement -> "実装"
    StepKind.Review -> "レビュー"
    StepKind.Checkpoint -> "チェックポイント"
    StepKind.Done -> "完了"
    StepKind.Unrecognized -> "不明"
}

/** Step の状態の言葉 (PC の `STEP_STATUS_LABEL`)。 */
fun stepStatusLabel(status: StepStatus): String = when (status) {
    StepStatus.Pending -> "待機"
    StepStatus.Running -> "実行中"
    StepStatus.Done -> "完了"
    StepStatus.Unrecognized -> "不明"
}

/** 調査は「実装」をコードではなく検索でする。 */
fun stepIcon(step: StepKind, kind: TaskKind): ImageVector = when (step) {
    StepKind.Implement -> if (kind == TaskKind.Investigate) YIcons.Search else YIcons.Code
    StepKind.Review -> YIcons.Eye
    StepKind.Checkpoint -> YIcons.Flag
    StepKind.Done, StepKind.Unrecognized -> YIcons.Check
}

fun groupIcon(status: GroupStatus, awaitingFinish: Boolean): ImageVector = when {
    awaitingFinish -> YIcons.Hourglass
    status == GroupStatus.Active -> YIcons.Layers
    status == GroupStatus.Finishing -> YIcons.Merge
    status == GroupStatus.Done -> YIcons.Check
    status == GroupStatus.MergeBlocked -> YIcons.Alert
    else -> YIcons.Ban
}

fun helpIcon(kind: HelpKind): ImageVector = when (kind) {
    HelpKind.Question, HelpKind.Policy -> YIcons.Help
    HelpKind.MergeConflict -> YIcons.Merge
    HelpKind.ReviewRoundsExhausted -> YIcons.Refresh
    HelpKind.AgentStopped -> YIcons.Stop
    HelpKind.GitFailed -> YIcons.Branch
    else -> YIcons.Alert
}

fun metaIcon(icon: MetaIcon): ImageVector = when (icon) {
    MetaIcon.GitMerge -> YIcons.Merge
    MetaIcon.Hourglass -> YIcons.Hourglass
}

/** 状態の系統の文字色と地 (モックの `.tone-*`)。 */
@Composable
fun toneChipColors(tone: Tone): Pair<Color, Color> {
    val c = Yhtye.colors
    return when (tone) {
        Tone.Waiting -> c.tx3 to c.bgChip
        Tone.Implementing -> c.implFg to c.implBg
        Tone.Reviewing -> c.revFg to c.revBg
        Tone.Handling -> c.handFg to c.handBg
        Tone.Done -> c.doneFg to c.doneBg
        Tone.Cancelled -> c.tx4 to c.bgChipDim
    }
}

/** グループの状態の色 (モックの `.gs-*`: 進行中 = 実装、マージ中 = レビュー、保留 = 対処、完了、中止)。 */
@Composable
fun groupChipColors(status: GroupStatus): Pair<Color, Color> = toneChipColors(
    when (status) {
        GroupStatus.Active -> Tone.Implementing
        GroupStatus.Finishing -> Tone.Reviewing
        GroupStatus.MergeBlocked -> Tone.Handling
        GroupStatus.Done -> Tone.Done
        else -> Tone.Cancelled
    },
)

/** 状態の小さなチップ: アイコン + 言葉。[active] のアイコンは呼吸する (エージェントが作業中)。 */
@Composable
fun StatusChip(
    icon: ImageVector,
    label: String,
    colors: Pair<Color, Color>,
    modifier: Modifier = Modifier,
    active: Boolean = false,
    labelTag: String? = null,
) {
    val (fg, bg) = colors
    Row(
        modifier
            .height(22.dp)
            .clip(CHIP_SHAPE)
            .background(bg)
            .padding(start = 5.dp, end = 7.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        YIcon(icon, fg, 13.dp, Modifier.alpha(if (active) breatheAlpha() else 1f))
        Text(
            label,
            color = fg,
            fontSize = 11.5.sp,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            style = CHIP_TEXT,
            modifier = if (labelTag != null) Modifier.testTag(labelTag) else Modifier,
        )
    }
}

/** タスクの状態のチップ。 */
@Composable
fun TaskStatusChip(task: Task, tone: Tone, modifier: Modifier = Modifier) {
    StatusChip(toneIcon(tone, task.kind), TONE_LABEL.getValue(tone), toneChipColors(tone), modifier, isActiveTone(tone), labelTag = "task-status-${task.id}")
}

/** Step の並び (アイコン、間を短い線でつなぐ。動いている Step は点滅)。 */
@Composable
fun StepsRow(task: Task, tone: Tone, modifier: Modifier = Modifier, size: Dp = 14.dp) {
    val c = Yhtye.colors
    Row(modifier.semantics { contentDescription = "steps" }, verticalAlignment = Alignment.CenterVertically) {
        task.steps.forEachIndexed { i, s ->
            if (i > 0) Box(Modifier.padding(horizontal = 3.dp).width(6.dp).height(1.dp).background(c.lineCard))
            // 終わったタスク (中止) の Step はコアでは実行中のまま残ることがある: 動いているようには見せない (モック)
            val running = s.status == StepStatus.Running && !isTerminal(task)
            val color = when {
                s.status == StepStatus.Done -> c.doneBar
                !running -> c.tx5
                tone == Tone.Reviewing -> c.revBar
                tone == Tone.Handling -> c.handBar
                else -> c.implBar
            }
            val pulse = running && tone != Tone.Handling
            val verdict = s.verdict?.let { if (it == Verdict.Approve) " · 承認" else " · 要修正" }.orEmpty()
            YIcon(
                stepIcon(s.kind, task.kind),
                color,
                size,
                Modifier
                    .alpha(if (pulse) breatheAlpha(periodMs = 1_400, min = 0.3f) else 1f)
                    .semantics { contentDescription = "${stepLabel(s.kind)} · ${stepStatusLabel(s.status)}$verdict" },
            )
        }
    }
}

/** 担当のセッションと作業時間 (`implementer · 14m`)。 */
@Composable
fun AgentLabel(agent: String, elapsed: String?, color: Color) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        YIcon(YIcons.Bot, color, 12.dp)
        Text(
            if (elapsed != null) "$agent · $elapsed" else agent,
            color = color,
            fontFamily = MonoFont,
            fontSize = 11.5.sp,
            maxLines = 1,
            modifier = Modifier.testTag("task-agent"),
        )
    }
}

/**
 * 対処中の help の文 (状態の表示だけ。扱うのはオーケストレータ)。種類はアイコンと読み上げの言葉、エージェントの
 * 処理が失われたらその印。
 */
@Composable
fun HelpNote(help: Help, textColor: Color, iconColor: Color, modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth().testTag("help-${help.id}"), horizontalArrangement = Arrangement.spacedBy(7.dp)) {
        Row(
            Modifier.padding(top = 3.dp).semantics { contentDescription = "${help.id} · ${helpKindLabel(help.kind)}" },
            horizontalArrangement = Arrangement.spacedBy(3.dp),
        ) {
            YIcon(helpIcon(help.kind), iconColor, 14.dp)
            if (help.agentLost) {
                Box(Modifier.semantics { contentDescription = "エージェント喪失" }) { YIcon(YIcons.Unplug, iconColor, 14.dp) }
            }
        }
        Text(help.message, color = textColor, fontSize = 13.5.sp, lineHeight = 21.sp, modifier = Modifier.weight(1f))
    }
}

/** 灰色の小さな 1 行 (中止の理由・待ちの理由・グループの結果)。 */
@Composable
fun MetaLine(icon: ImageVector?, text: String, color: Color, modifier: Modifier = Modifier, description: String? = null) {
    Row(
        modifier.then(if (description != null) Modifier.semantics { contentDescription = description } else Modifier),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (icon != null) YIcon(icon, color, 12.dp)
        Text(text, color = color, fontFamily = MonoFont, fontSize = 11.5.sp, lineHeight = 16.sp)
    }
}

/** PC と同じ「再起動で中断されました」の注記。 */
const val INTERRUPTED_NOTE = "再起動で中断されました。自動で再開します。"

/** ■ で止める相手 (確認のダイアログに出す)。 */
sealed interface CancelTarget {
    val id: String

    data class OfTask(override val id: String) : CancelTarget

    data class OfGroup(override val id: String) : CancelTarget

    /** 画面の作り直しで残すための文字列。 */
    val key: String get() = when (this) {
        is OfTask -> "task:$id"
        is OfGroup -> "group:$id"
    }

    companion object {
        fun parse(key: String): CancelTarget? = when {
            key.startsWith("task:") -> OfTask(key.removePrefix("task:"))
            key.startsWith("group:") -> OfGroup(key.removePrefix("group:"))
            else -> null
        }
    }
}

/**
 * ■ の確認 (タッチの誤操作を防ぐ。PC は確認なし)。言葉は PC のボタンの説明と同じ。
 */
@Composable
fun CancelConfirmDialog(target: CancelTarget, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    val what = when (target) {
        is CancelTarget.OfTask -> "エージェントを停止 (タスクを中止)"
        is CancelTarget.OfGroup -> "グループを中止"
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("${target.id} を中止しますか?") },
        text = { Text(what) },
        confirmButton = {
            TextButton(onClick = onConfirm, modifier = Modifier.testTag("cancel-confirm")) { Text("中止する", color = Yhtye.colors.handBar) }
        },
        dismissButton = { TextButton(onClick = onDismiss, modifier = Modifier.testTag("cancel-dismiss")) { Text("キャンセル") } },
    )
}
