package io.github.tom039224.yhtye.ui.dashboard

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.UsageStats
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.sync.AppState
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.DEFAULT_STATS_DAYS
import io.github.tom039224.yhtye.shared.sync.barBucketSize
import io.github.tom039224.yhtye.shared.sync.bucketDays
import io.github.tom039224.yhtye.ui.common.EmptyState
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons
import io.github.tom039224.yhtye.ui.common.rememberActionScope
import kotlinx.coroutines.launch

// ダッシュボードの利用統計 (PC の UsageStatsSection): ホストのエージェントが使ったトークン (`get_usage_stats`。各エージェントの
// セッションログから数えた実績)。期間の切替・合計・日別の棒・内訳・ヒートマップ。見えたときと期間を変えたときに読み、
// 読んでいる間は前の結果を薄くして残す。

private val SEGMENT_SHAPE = RoundedCornerShape(8.dp)
private val OPTION_SHAPE = RoundedCornerShape(6.dp)
private val BOX_SHAPE_RADIUS = 10.dp

/** 読み直している間の古いレポートの薄さ (PC の 55%)。 */
private const val STALE_ALPHA = 0.55f

/** グラフを横に並べる幅 (これより狭ければ縦に積む)。 */
private val WIDE_STATS = 600.dp

@Composable
fun UsageStatsSection(app: AppState, sync: AppStore) {
    val c = Yhtye.colors
    val scope = rememberActionScope()
    val stats = app.stats
    val connected = app.connection is ConnectionState.Connected
    val days = stats.days.takeIf { it in PERIODS } ?: DEFAULT_STATS_DAYS
    // 見えたとき (と、つながり直したとき) に読む。期間を変えたときはその場で読む
    LaunchedEffect(connected) { if (connected) sync.refreshStats(days) }
    Column(Modifier.fillMaxWidth().semantics { contentDescription = "利用統計" }.testTag("usage-stats"), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        StatsHead(
            hostName = app.host.name,
            days = days,
            connected = connected,
            loading = stats.loading,
            onPeriod = { p -> scope.launch { sync.refreshStats(p) } },
            onRefresh = { scope.launch { sync.refreshStats(days, refresh = true) } },
        )
        val loading = stats.loading || !connected
        val report = stats.report
        val error = stats.error
        when {
            report != null -> StatsReport(report, dimmed = loading || stats.isStale)
            loading || error == null -> StatsLoading()
            else -> StatsError(error) { scope.launch { sync.refreshStats(days, refresh = true) } }
        }
    }
}

/**
 * 見出し・ホスト名のチップ・期間の切替・再読み込み。広ければ 1 行 (PC と同じ)、狭ければ操作を次の行の右端に回す
 * (ホスト名を潰さない)。
 */
@Composable
private fun StatsHead(hostName: String?, days: Int, connected: Boolean, loading: Boolean, onPeriod: (Int) -> Unit, onRefresh: () -> Unit) {
    val c = Yhtye.colors
    val controls: @Composable () -> Unit = {
        PeriodSelect(days, onPeriod)
        IconButton(
            onClick = onRefresh,
            enabled = connected,
            modifier = Modifier.size(40.dp).semantics { contentDescription = "再読み込み" }.testTag("stats-refresh"),
        ) {
            YIcon(YIcons.Refresh, if (connected) c.tx3 else c.tx5, 18.dp, spin = loading)
        }
    }
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val wide = maxWidth >= WIDE_STATS
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SectionHeading(YIcons.Activity, "利用統計")
                // ホスト名は残りの幅に収める (長ければ省略)
                Box(Modifier.weight(1f)) { hostName?.let { HostChip(it) } }
                if (wide) controls()
            }
            if (!wide) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
                    controls()
                }
            }
        }
    }
}

/** 節の見出し: アイコン + 見出し (PC の `.dash-heading`)。 */
@Composable
fun SectionHeading(icon: androidx.compose.ui.graphics.vector.ImageVector, text: String) {
    val c = Yhtye.colors
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        YIcon(icon, c.tx4, 17.dp)
        Text(text, color = c.tx, fontSize = 16.sp, fontWeight = FontWeight.Medium, maxLines = 1)
    }
}

@Composable
private fun HostChip(name: String) {
    val c = Yhtye.colors
    Text(
        name,
        color = c.tx4,
        fontFamily = MonoFont,
        fontSize = 11.sp,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier.clip(RoundedCornerShape(4.dp)).background(c.bgChipDim).padding(horizontal = 7.dp, vertical = 2.dp).testTag("stats-host"),
    )
}

/** 期間の切替: 1 つの枠の中のセグメント (選ばれたものは地 `bgChip`・文字 `accent`)。 */
@Composable
private fun PeriodSelect(days: Int, onSelect: (Int) -> Unit) {
    val c = Yhtye.colors
    Row(
        Modifier
            .clip(SEGMENT_SHAPE)
            .border(1.dp, c.lineCard, SEGMENT_SHAPE)
            .background(c.bgInset)
            .padding(2.dp)
            .semantics { contentDescription = "期間" },
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        PERIODS.forEach { p ->
            val active = p == days
            Text(
                "${p}日",
                color = if (active) c.accent else c.tx4,
                fontFamily = MonoFont,
                fontSize = 12.sp,
                fontWeight = FontWeight.Medium,
                modifier = Modifier
                    .clip(OPTION_SHAPE)
                    .background(if (active) c.bgChip else c.bgInset)
                    .clickable(role = Role.RadioButton) { onSelect(p) }
                    .semantics { selected = active }
                    .padding(horizontal = 10.dp, vertical = 7.dp)
                    .testTag("period-$p"),
            )
        }
    }
}

/** 読み込み中・ログ無し・利用なし・失敗の置き場所: レポートの位置に破線の箱 (高さ 200dp 以上)。 */
@Composable
fun DashedBox(modifier: Modifier = Modifier, minHeight: androidx.compose.ui.unit.Dp = 200.dp, content: @Composable () -> Unit) {
    val c = Yhtye.colors
    Column(
        modifier
            .fillMaxWidth()
            .heightIn(min = minHeight)
            .drawBehind {
                val w = 1.dp.toPx()
                drawRoundRect(
                    c.lineDashed,
                    topLeft = androidx.compose.ui.geometry.Offset(w / 2, w / 2),
                    size = androidx.compose.ui.geometry.Size(size.width - w, size.height - w),
                    cornerRadius = CornerRadius(BOX_SHAPE_RADIUS.toPx()),
                    style = Stroke(w, pathEffect = PathEffect.dashPathEffect(floatArrayOf(4.dp.toPx(), 3.dp.toPx()))),
                )
            }
            .padding(20.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp, Alignment.CenterVertically),
    ) { content() }
}

@Composable
private fun StatsLoading() {
    val c = Yhtye.colors
    DashedBox(Modifier.semantics { liveRegion = LiveRegionMode.Polite }.testTag("usage-loading")) {
        YIcon(YIcons.Loader, c.accent, 18.dp, spin = true)
        Text("読み込み中…", color = c.tx4, fontSize = 13.sp)
    }
}

@Composable
private fun StatsError(error: String, onRetry: () -> Unit) {
    val c = Yhtye.colors
    DashedBox(Modifier.testTag("usage-error")) {
        EmptyState(YIcons.Alert, "利用統計を読めませんでした。", error = true, modifier = Modifier.padding(0.dp))
        Text(error, color = c.handFg, fontFamily = MonoFont, fontSize = 11.5.sp, textAlign = TextAlign.Center)
        DashButton("再試行", onClick = onRetry, tag = "stats-retry")
    }
}

@Composable
private fun StatsReport(report: UsageStats, dimmed: Boolean) {
    val c = Yhtye.colors
    if (noLogsFound(report)) {
        DashedBox(Modifier.testTag("usage-no-logs")) {
            EmptyState(YIcons.Activity, "エージェントのログが見つかりません。")
            Text(
                "${report.sources.joinToString("・") { harnessLabel(it.harness) }} のセッションログがこのホストにありません。エージェントを使うと、ここに集計されます。",
                color = c.tx4,
                fontSize = 12.5.sp,
                lineHeight = 20.sp,
                textAlign = TextAlign.Center,
            )
        }
        return
    }
    if (noUsage(report)) {
        DashedBox(Modifier.testTag("usage-none")) { EmptyState(YIcons.Activity, "この ${report.periodDays} 日間の利用はありません。") }
        return
    }
    val order = harnessOrder(report)
    val bars = barSeries(bucketDays(report.days, barBucketSize(report.periodDays.toInt())), order)
    BoxWithConstraints(Modifier.fillMaxWidth().alpha(if (dimmed) STALE_ALPHA else 1f).testTag("usage-report")) {
        val wide = maxWidth >= WIDE_STATS
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            StatCards(report, wide)
            Adaptive(wide) {
                DashPanel("日別のトークン数", it(3f)) { DailyChart(bars, order) }
                DashPanel("アクティビティ", it(2f)) { Heatmap(report) }
            }
            Adaptive(wide) {
                Breakdown("モデル別", breakdownRows(report.byModel), it(1f))
                Breakdown("ハーネス別", breakdownRows(report.byHarness, ::harnessLabel, order), it(1f))
                Breakdown("プロジェクト別", projectRows(report.byProject), it(1f))
            }
        }
    }
}

/** 広ければ横に (重み [weight] で分け合う)、狭ければ縦に積む。中身は自分の幅の Modifier を `it(weight)` で受ける。 */
@Composable
private fun Adaptive(wide: Boolean, content: @Composable ((Float) -> Modifier) -> Unit) {
    if (wide) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) { content { w -> Modifier.weight(w) } }
    } else {
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(12.dp)) { content { Modifier.fillMaxWidth() } }
    }
}

/** ダッシュボードのボタン (PC の `.dash-button`)。[primary] は琥珀の主ボタン。 */
@Composable
fun DashButton(
    text: String,
    onClick: () -> Unit,
    tag: String,
    modifier: Modifier = Modifier,
    primary: Boolean = false,
    enabled: Boolean = true,
    icon: androidx.compose.ui.graphics.vector.ImageVector? = null,
    spin: Boolean = false,
) {
    val c = Yhtye.colors
    val shape = RoundedCornerShape(8.dp)
    val fg = if (primary) c.accentStrong else c.tx2
    Row(
        modifier
            .heightIn(min = 40.dp)
            .clip(shape)
            .border(1.dp, if (primary) c.accent.copy(alpha = 0.55f) else c.lineCard, shape)
            .background(if (primary) c.accentSoft else c.bgInset)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .alpha(if (enabled) 1f else 0.5f)
            .padding(horizontal = 14.dp)
            .testTag(tag),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (icon != null) YIcon(icon, fg, 16.dp, spin = spin)
        Text(text, color = fg, fontSize = 14.sp, fontWeight = if (primary) FontWeight.Medium else FontWeight.Normal, maxLines = 1)
    }
}
