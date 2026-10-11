package io.github.tom039224.yhtye.ui.panels

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
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
import io.github.tom039224.yhtye.shared.api.generated.UsageWindow
import io.github.tom039224.yhtye.shared.api.generated.UsageWindowKind
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.HostTone
import io.github.tom039224.yhtye.shared.sync.formatPingAge
import io.github.tom039224.yhtye.shared.sync.formatRemaining
import io.github.tom039224.yhtye.shared.sync.hostTone
import io.github.tom039224.yhtye.shared.sync.hostToneLabel
import io.github.tom039224.yhtye.shared.sync.pingAgeSeconds
import io.github.tom039224.yhtye.shared.sync.planLabel
import io.github.tom039224.yhtye.shared.sync.usageWindow
import io.github.tom039224.yhtye.ui.common.EmptyState
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.rememberActionScope
import io.github.tom039224.yhtye.ui.common.YIcons
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

// 左パネル (PC のアイコンレール + サイドバー + ステータスバーの使用量、モックの `#left`): 左端のレールでコーディング /
// 履歴を切り替え、一番下に設定。コーディングではプロジェクトのドロップダウン・バックグラウンドで動作中のプロジェクト・
// ブランチとチャットの木。下端にコアのホスト (PC のサイドバーのフッタ) とプラン・使用量 (PC のステータスバー)。

private val RAIL_WIDTH = 56.dp
private val RAIL_BUTTON_SHAPE = RoundedCornerShape(12.dp)
private val METER_SHAPE = RoundedCornerShape(2.dp)

/** ホストの最後の ping からの経過を数え直す間隔 (PC の useNow(1000))。 */
private const val HOST_TICK_MS = 1_000L

/** PC のアイコンレールのビュー。 */
enum class MainView { Work, Runs }

/** 履歴のビュー (PC の RunsView): まだ設計していないので PC と同じ空の状態。 */
const val RUNS_EMPTY = "現状は何もありません"

/**
 * @param onChatChosen チャットを選んだ・作った (中央に戻る)。
 * @param onOpenSettings レールの一番下の設定。
 */
@Composable
fun LeftPanel(
    app: AppState,
    sync: AppStore,
    now: Long,
    view: MainView,
    onView: (MainView) -> Unit,
    memory: LeftPanelMemory,
    onChatChosen: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    val c = Yhtye.colors
    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars).testTag("left-panel")) {
        Row(Modifier.weight(1f).fillMaxWidth()) {
            Rail(view, onView, onOpenSettings)
            Box(Modifier.weight(1f).fillMaxHeight()) {
                when (view) {
                    MainView.Runs -> EmptyState(YIcons.History, RUNS_EMPTY, Modifier.align(Alignment.Center), testTag = "runs-empty")
                    MainView.Work -> WorkSide(app, sync, now, memory, onChatChosen)
                }
            }
        }
        HorizontalDivider(color = c.line)
        Footer(app, sync, now)
    }
}

/** 左端のレール: コーディング / 履歴、一番下に設定。 */
@Composable
private fun Rail(view: MainView, onView: (MainView) -> Unit, onOpenSettings: () -> Unit) {
    val c = Yhtye.colors
    Column(
        Modifier.width(RAIL_WIDTH).fillMaxHeight().background(c.bgRail).padding(vertical = 10.dp).semantics { contentDescription = "views" },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        RailButton(YIcons.Code, "コーディング", view == MainView.Work, "rail-work") { onView(MainView.Work) }
        RailButton(YIcons.History, "履歴", view == MainView.Runs, "rail-runs") { onView(MainView.Runs) }
        Spacer(Modifier.weight(1f))
        RailButton(YIcons.Sliders, "設定", active = false, tag = "rail-settings", onClick = onOpenSettings)
    }
}

@Composable
private fun RailButton(icon: ImageVector, label: String, active: Boolean, tag: String, onClick: () -> Unit) {
    val c = Yhtye.colors
    Box(contentAlignment = Alignment.CenterStart) {
        if (active) {
            Box(Modifier.offset(x = (-6).dp).width(3.dp).height(18.dp).clip(RoundedCornerShape(topEnd = 2.dp, bottomEnd = 2.dp)).background(c.accent))
        }
        Box(
            Modifier
                .size(44.dp)
                .clip(RAIL_BUTTON_SHAPE)
                .then(if (active) Modifier.background(c.bgChip) else Modifier)
                .clickable(role = Role.Button, onClick = onClick)
                .semantics {
                    contentDescription = label
                    selected = active
                }
                .testTag(tag),
            contentAlignment = Alignment.Center,
        ) { YIcon(icon, if (active) c.accent else c.tx4, 20.dp) }
    }
}

/** コーディングのビューの中身: プロジェクト、バックグラウンドのプロジェクト、ブランチとチャット。 */
@Composable
private fun WorkSide(app: AppState, sync: AppStore, now: Long, memory: LeftPanelMemory, onChatChosen: () -> Unit) {
    ProjectPickerArea(app, sync) {
        BackgroundProjects(app, sync)
        BranchTree(app, sync, now, memory, onChatChosen)
    }
}

/** 下端: コアのホストと最後の ping からの経過、プランと使用量 (タップで取り直す)。 */
@Composable
private fun Footer(app: AppState, sync: AppStore, now: Long) {
    val c = Yhtye.colors
    val scope = rememberActionScope()
    Column(
        Modifier
            .fillMaxWidth()
            .background(c.bgRail)
            .windowInsetsPadding(WindowInsets.navigationBars)
            .padding(start = 14.dp, end = 14.dp, top = 9.dp, bottom = 10.dp),
        verticalArrangement = Arrangement.spacedBy(7.dp),
    ) {
        HostRow(app)
        val report = app.usage.report
        val enabled = app.connection is ConnectionState.Connected && !app.usage.loading
        FlowRow(
            Modifier
                .fillMaxWidth()
                .clickable(enabled = enabled, role = Role.Button, onClickLabel = "使用量を取り直す") { scope.launch { sync.refreshUsage(true) } }
                .semantics { contentDescription = "プランと使用量" + (app.usage.error?.let { ": 使用量を取得できません: $it" } ?: "") }
                .testTag("usage"),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
            itemVerticalAlignment = Alignment.CenterVertically,
        ) {
            report?.plan?.let { plan ->
                Text(planLabel(plan), color = c.tx3, fontFamily = MonoFont, fontSize = 11.5.sp, modifier = Modifier.testTag("plan"))
                Box(Modifier.width(1.dp).height(12.dp).background(c.lineCard))
            }
            UsageMeter("5h", "5h", c.usage5h, usageWindow(report, UsageWindowKind.FiveHour), now)
            UsageMeter("week", "7d", c.usageWeek, usageWindow(report, UsageWindowKind.Week), now)
        }
    }
}

@Composable
private fun HostRow(app: AppState) {
    val c = Yhtye.colors
    var tick by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(HOST_TICK_MS)
            tick = System.currentTimeMillis()
        }
    }
    val age = pingAgeSeconds(tick, app.host.lastPingAt)
    val tone = hostTone(app.connection, age)
    val state = hostToneLabel(tone, app.connection)
    val dot = when (tone) {
        HostTone.Ok -> c.doneBar
        HostTone.Busy -> c.accent
        HostTone.Bad -> c.handBar
    }
    Row(Modifier.fillMaxWidth().testTag("host"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(7.dp)) {
        Box(Modifier.size(7.dp).background(dot, CircleShape).semantics { contentDescription = state }.testTag("host-dot"))
        Text(
            app.host.name ?: "—",
            color = c.tx2,
            fontFamily = MonoFont,
            fontSize = 11.5.sp,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        YIcon(YIcons.Clock, c.tx3, 12.dp)
        Text(
            age?.let(::formatPingAge) ?: "—",
            color = c.tx3,
            fontFamily = MonoFont,
            fontSize = 11.5.sp,
            modifier = Modifier.semantics { contentDescription = if (age == null) "まだ応答がありません" else "最後の ping からの経過" },
        )
    }
}

/** 使用量の 1 つの窓: 名前、バー、割合、リセットまで (報告が無ければ空のまま「—」。値を作らない)。 */
@Composable
private fun UsageMeter(id: String, label: String, color: Color, window: UsageWindow?, now: Long) {
    val c = Yhtye.colors
    val percent = window?.percent?.coerceIn(0.0, 100.0)
    Row(Modifier.testTag("usage-$id"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
        Text(label, color = c.tx3, fontFamily = MonoFont, fontSize = 11.5.sp)
        Box(Modifier.width(40.dp).height(4.dp).clip(METER_SHAPE).background(c.lineCard)) {
            if (percent != null) Box(Modifier.fillMaxWidth((percent / 100).toFloat()).fillMaxHeight().clip(METER_SHAPE).background(color))
        }
        Text(percent?.let { "${it.roundToInt()}%" } ?: "—", color = c.tx3, fontFamily = MonoFont, fontSize = 11.5.sp)
        window?.resetsAtMs?.let { at ->
            Row(
                Modifier.semantics(mergeDescendants = true) { contentDescription = "リセットまで" },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                YIcon(YIcons.Hourglass, c.tx4, 12.dp)
                Text(formatRemaining(at - now), color = c.tx4, fontFamily = MonoFont, fontSize = 11.5.sp)
            }
        }
    }
}
