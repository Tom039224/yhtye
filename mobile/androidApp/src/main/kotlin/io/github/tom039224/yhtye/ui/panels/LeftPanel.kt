package io.github.tom039224.yhtye.ui.panels

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.lerp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.UsageWindow
import io.github.tom039224.yhtye.shared.api.generated.UsageWindowKind
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.formatRemaining
import io.github.tom039224.yhtye.shared.sync.planLabel
import io.github.tom039224.yhtye.shared.sync.usageWindow
import io.github.tom039224.yhtye.ui.common.EmptyState
import io.github.tom039224.yhtye.ui.common.SwipeThresholds
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.common.rememberActionScope
import io.github.tom039224.yhtye.ui.shell.LocalPanelShell
import io.github.tom039224.yhtye.ui.shell.Panel
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

// 左パネル (PC のアイコンレール + サイドバー + ステータスバーの使用量、モックの `#left`): 左端のレール (`Rail.kt`) で
// ダッシュボード / 開いているプロジェクト / コーディング / 履歴を切り替え、一番下に設定。レールは左パネルが開いているときの
// もう 1 回の左→右のスワイプで広がり、プロジェクトの名前が出る (`RailExpansion.kt`)。その右にプロジェクトの名前と
// ブランチとチャットの木 (コーディングのときだけ。PC もダッシュボードと履歴ではサイドバーを隠す。ダッシュボードはレールだけ、
// 履歴は PC の RunsView と同じ空の状態)。下端にサーバーとクライアントの一覧 (PC のサイドバーのフッタ、`HostFooter.kt`。
// サイドバーと同じくコーディングのときだけ) とプラン・使用量 (PC のステータスバー。どのビューでも出す)。

private val METER_SHAPE = RoundedCornerShape(2.dp)

/** PC のアイコンレールのビュー。アプリの最初はダッシュボード。 */
enum class MainView { Dashboard, Work, Runs }

/** 履歴のビュー (PC の RunsView): まだ設計していないので PC と同じ空の状態。 */
const val RUNS_EMPTY = "現状は何もありません"

/**
 * @param selfId ペアリングで保存したこの端末の ID (下端の一覧で「この端末」の印を付ける)。
 * @param onChatChosen チャットを選んだ・作った (中央に戻る)。
 * @param onOpenSettings レールの一番下の設定。
 * @param updateAvailable アプリの新しい版がある (設定のボタンに小さな印を付ける)。
 * @param gestureInsets 画面の端のこの幅から始まったスワイプではレールを広げない (Android の「戻る」。パネルと同じ)。
 */
@Composable
fun LeftPanel(
    app: AppState,
    sync: AppStore,
    now: Long,
    view: MainView,
    onView: (MainView) -> Unit,
    memory: LeftPanelMemory,
    selfId: String?,
    onChatChosen: () -> Unit,
    onOpenSettings: () -> Unit,
    updateAvailable: Boolean = false,
    gestureInsets: WindowInsets = WindowInsets(0),
) {
    val c = Yhtye.colors
    val expansion = rememberRailExpansion()
    // パネルが閉じ始めたら畳む (次に開いたときは畳んだレール)。戻るは広がったレールを先に畳む
    val shell = LocalPanelShell.current
    val panelShown = shell == null || shell.wide || shell.open == Panel.Left
    LaunchedEffect(panelShown) { if (!panelShown) expansion.collapse() }
    BackHandler(enabled = expansion.open) { expansion.collapse() }
    val density = LocalDensity.current
    val haptic = LocalHapticFeedback.current
    val edgeStart = gestureInsets.getLeft(density, LocalLayoutDirection.current).toFloat()
    val fling = with(density) { SwipeThresholds.FLING_DP_PER_SECOND.dp.toPx() }
    Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars).testTag("left-panel")) {
        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
            val expandedWidth = minOf(RAIL_EXPANDED_WIDTH, maxWidth)
            val distance = with(density) { (expandedWidth - RAIL_WIDTH).toPx() }
            // 中身・レール・外のタップの受け手のどれの上から始まったスワイプでも、ここで受ける (子が取らなかった横の動き)
            Box(Modifier.fillMaxSize().pointerInput(expansion, distance, fling, edgeStart) { railSwipe(expansion, distance, fling, edgeStart, haptic) }) {
                Row(Modifier.fillMaxSize()) {
                    Spacer(Modifier.width(RAIL_WIDTH).fillMaxHeight())
                    Box(Modifier.weight(1f).fillMaxHeight()) {
                        when (view) {
                            MainView.Runs -> EmptyState(YIcons.History, RUNS_EMPTY, Modifier.align(Alignment.Center), testTag = "runs-empty")
                            MainView.Work -> WorkSide(app, sync, now, memory, onChatChosen)
                            MainView.Dashboard -> Unit // レールだけ (PC のダッシュボードにサイドバーは無い)
                        }
                    }
                }
                RailOverlay(app, sync, view, onView, onOpenSettings, updateAvailable, expansion, expandedWidth)
            }
        }
        HorizontalDivider(color = c.line)
        Footer(app, sync, now, selfId, showHosts = view == MainView.Work)
    }
}

/** 中身の上に重なるレール。広がっている間はその外のタップで畳む (PC でポインタがレールを離れたとき)。 */
@Composable
private fun RailOverlay(
    app: AppState,
    sync: AppStore,
    view: MainView,
    onView: (MainView) -> Unit,
    onOpenSettings: () -> Unit,
    updateAvailable: Boolean,
    expansion: RailExpansion,
    expandedWidth: Dp,
) {
    val progress = expansion.progress
    if (progress > 0f) {
        Box(Modifier.fillMaxSize().pointerInput(expansion) { detectTapGestures { expansion.collapse() } }.testTag("rail-outside"))
    }
    Rail(
        app = app,
        sync = sync,
        view = view,
        onView = onView,
        onProjectChosen = {
            onView(MainView.Work)
            expansion.collapse()
        },
        onOpenSettings = onOpenSettings,
        updateAvailable = updateAvailable,
        width = lerp(RAIL_WIDTH, expandedWidth, progress),
        showLabels = progress > 0f,
    )
}

/** コーディングのパネルの中身: 表示中のプロジェクトの名前と、ブランチとチャットの木。 */
@Composable
private fun WorkSide(app: AppState, sync: AppStore, now: Long, memory: LeftPanelMemory, onChatChosen: () -> Unit) {
    ProjectPickerArea(app) {
        BranchTree(app, sync, now, memory, onChatChosen)
    }
}

/** 下端: サーバー (コアのホストと最後の ping からの経過) と端末の一覧 ([showHosts] のときだけ)、プランと使用量 (タップで取り直す)。 */
@Composable
private fun Footer(app: AppState, sync: AppStore, now: Long, selfId: String?, showHosts: Boolean) {
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
        if (showHosts) HostAndClients(app, selfId)
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
