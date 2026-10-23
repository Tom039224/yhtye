package io.github.tom039224.yhtye.ui.dashboard

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.UsageStats
import io.github.tom039224.yhtye.shared.sync.HEAT_LEVELS
import io.github.tom039224.yhtye.shared.sync.HeatCell
import io.github.tom039224.yhtye.shared.sync.heatmapWeeks
import kotlin.math.roundToInt

// 利用統計のグラフ (PC の `UsageCharts.tsx`。チャートのライブラリは使わず Compose の Canvas で描く)。色はテーマの
// `chartSeries` (ハーネス。同じ番号 = 同じ色) と `heat` (ヒートマップの段)。PC のホバーのツールチップはタップで出す
// (選んだ棒・マスの詳しい値を下の 1 行に)。

private val CARD_SHAPE = RoundedCornerShape(10.dp)
private val BAR_TRACK_SHAPE = RoundedCornerShape(3.dp)
private val SWATCH_SHAPE = RoundedCornerShape(2.dp)

/** 日別のグラフの高さ、左の目盛りの幅、棒の間。 */
private val CHART_HEIGHT = 160.dp
private val AXIS_WIDTH = 36.dp
private val BAR_GAP = 2.dp

/** 選んでいない棒の薄さ (PC のホバーで他の日が沈む 45%)。 */
private const val DIMMED_BAR_ALPHA = 0.45f

/** ヒートマップのマスの大きさ (9〜18dp)、マスの間、曜日の文字の幅。 */
private val HEAT_MIN = 9.dp
private val HEAT_MAX = 18.dp
private val HEAT_GAP = 3.dp
private val WEEKDAY_WIDTH = 18.dp
private val WEEKDAYS = listOf("日", "月", "火", "水", "木", "金", "土")

/** 曜日の文字を出す行 (1 行おき、混まないように)。 */
private val LABELLED_WEEKDAYS = setOf(1, 3, 5)

/** パネル (地 `bgCard`・角 10dp の枠) に見出しと中身。 */
@Composable
fun DashPanel(title: String, modifier: Modifier = Modifier, testTag: String? = null, content: @Composable () -> Unit) {
    val c = Yhtye.colors
    Column(
        modifier
            .clip(CARD_SHAPE)
            .border(1.dp, c.lineCard, CARD_SHAPE)
            .background(c.bgCard)
            .padding(14.dp)
            .semantics { contentDescription = title }
            .then(if (testTag != null) Modifier.testTag(testTag) else Modifier),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(title, color = c.tx2, fontSize = 13.sp, fontWeight = FontWeight.Medium)
        content()
    }
}

/** 期間の合計: 合計トークン・入力・出力・キャッシュ読み込み・セッション。狭ければ合計を 1 行に、残りを 2 列に。 */
@Composable
fun StatCards(report: UsageStats, wide: Boolean) {
    val t = report.totals.tokens
    val cards = listOf(
        Triple("total", "合計トークン", formatTokens(t.total)),
        Triple("input", "入力", formatTokens(t.input)),
        Triple("output", "出力", formatTokens(t.output)),
        Triple("cache-read", "キャッシュ読み込み", formatTokens(t.cacheRead)),
        Triple("sessions", "セッション", formatExact(report.totals.sessions)),
    )
    val gap = 8.dp
    if (wide) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(gap)) {
            cards.forEach { (key, label, value) -> StatCard(key, label, value, Modifier.weight(1f)) }
        }
        return
    }
    Column(verticalArrangement = Arrangement.spacedBy(gap)) {
        val (key, label, value) = cards.first()
        StatCard(key, label, value, Modifier.fillMaxWidth())
        cards.drop(1).chunked(2).forEach { pair ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(gap)) {
                pair.forEach { (k, l, v) -> StatCard(k, l, v, Modifier.weight(1f)) }
            }
        }
    }
}

@Composable
private fun StatCard(key: String, label: String, value: String, modifier: Modifier) {
    val c = Yhtye.colors
    Column(
        modifier
            .clip(CARD_SHAPE)
            .border(1.dp, c.lineCard, CARD_SHAPE)
            .background(c.bgCard)
            .padding(horizontal = 14.dp, vertical = 12.dp)
            .semantics(mergeDescendants = true) {}
            .testTag("stat-$key"),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(label, color = c.tx4, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        // 合計トークンの値だけ琥珀
        Text(
            value,
            color = if (key == "total") c.accentStrong else c.tx,
            fontFamily = MonoFont,
            fontSize = 22.sp,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
        )
    }
}

/** 日別 (まとめた日) のトークン数をハーネス別に積んだ棒グラフ。タップした棒の内訳を下に出し、他の棒を沈める。 */
@Composable
fun DailyChart(bars: List<BarSeries>, order: List<String>) {
    val c = Yhtye.colors
    val top = niceCeil(bars.maxOfOrNull { it.bucket.tokens } ?: 0)
    var selected by remember(bars) { mutableStateOf<Int?>(null) }
    val used = order.filter { h -> bars.any { b -> b.segments.any { it.first == h } } }
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(Modifier.fillMaxWidth().height(CHART_HEIGHT), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Column(Modifier.width(AXIS_WIDTH).fillMaxHeight(), horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.SpaceBetween) {
                listOf(formatTokens(top), formatTokens(top / 2), "0").forEach { AxisText(it) }
            }
            Canvas(
                Modifier
                    .weight(1f)
                    .fillMaxHeight()
                    .pointerInput(bars) {
                        detectTapGestures { pos ->
                            if (bars.isEmpty()) return@detectTapGestures
                            val i = (pos.x / (size.width.toFloat() / bars.size)).toInt().coerceIn(0, bars.lastIndex)
                            selected = if (selected == i) null else i
                        }
                    }
                    .semantics { contentDescription = "日別のトークン数 (${bars.size} 本、ハーネス別)" }
                    .testTag("daily-chart"),
            ) {
                val dash = PathEffect.dashPathEffect(floatArrayOf(2.dp.toPx(), 3.dp.toPx()))
                val stroke = 1.dp.toPx()
                drawLine(c.line, Offset(0f, stroke / 2), Offset(size.width, stroke / 2), stroke, pathEffect = dash)
                drawLine(c.line, Offset(0f, size.height / 2), Offset(size.width, size.height / 2), stroke, pathEffect = dash)
                if (bars.isEmpty()) return@Canvas
                val slot = size.width / bars.size
                val gap = minOf(BAR_GAP.toPx(), slot / 3)
                bars.forEachIndexed { i, bar ->
                    val alpha = if (selected == null || selected == i) 1f else DIMMED_BAR_ALPHA
                    if (selected == i) drawRect(c.bgPressed.copy(alpha = 0.5f), Offset(i * slot, 0f), Size(slot, size.height))
                    var used0 = 0f
                    bar.segments.forEach { (harness, tokens) ->
                        val h = tokens.toFloat() / top * size.height
                        drawRect(
                            c.chartSeries[seriesIndex(order, harness)].copy(alpha = alpha),
                            Offset(i * slot + gap / 2, size.height - used0 - h),
                            Size(slot - gap, h),
                        )
                        used0 += h
                    }
                }
                drawLine(c.lineDashed, Offset(0f, size.height - stroke / 2), Offset(size.width, size.height - stroke / 2), stroke)
            }
        }
        Row(Modifier.fillMaxWidth().padding(start = AXIS_WIDTH + 6.dp), horizontalArrangement = Arrangement.SpaceBetween) {
            AxisText(bars.firstOrNull()?.let { shortDate(it.bucket.from) }.orEmpty())
            AxisText(bars.lastOrNull()?.let { shortDate(it.bucket.to) }.orEmpty())
        }
        selected?.let { bars.getOrNull(it) }?.let { bar -> BarDetail(bar) }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            used.forEach { h ->
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Swatch(c.chartSeries[seriesIndex(order, h)])
                    Text(harnessLabel(h), color = c.tx3, fontFamily = MonoFont, fontSize = 12.sp)
                }
            }
        }
    }
}

/** 選んだ棒の詳しい値 (PC の日のツールチップ: 期間・合計・セッション・ハーネス別)。 */
@Composable
private fun BarDetail(bar: BarSeries) {
    val c = Yhtye.colors
    val b = bar.bucket
    val lines = listOf("${bucketLabel(b)}: ${formatExact(b.tokens)} トークン · ${b.sessions} セッション") +
        bar.segments.map { (h, tokens) -> "${harnessLabel(h)}: ${formatExact(tokens)}" }
    Text(lines.joinToString("\n"), color = c.tx2, fontFamily = MonoFont, fontSize = 11.5.sp, lineHeight = 17.sp, modifier = Modifier.testTag("bar-detail"))
}

@Composable
private fun AxisText(text: String) {
    Text(text, color = Yhtye.colors.tx4, fontFamily = MonoFont, fontSize = 10.5.sp, maxLines = 1)
}

@Composable
private fun Swatch(color: Color) {
    Box(Modifier.size(9.dp).clip(SWATCH_SHAPE).background(color))
}

/** 内訳 (モデル別・ハーネス別・プロジェクト別): 名前・トークン・割合、その下に割合の棒、その下にセッション数。 */
@Composable
fun Breakdown(title: String, rows: List<BreakdownRow>, modifier: Modifier = Modifier) {
    val c = Yhtye.colors
    DashPanel(title, modifier) {
        if (rows.isEmpty()) {
            Text("データはありません。", color = c.tx4, fontSize = 12.sp)
            return@DashPanel
        }
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            rows.forEach { r -> BreakdownLine(r) }
        }
    }
}

@Composable
private fun BreakdownLine(r: BreakdownRow) {
    val c = Yhtye.colors
    // ハーネスはその色、モデル・プロジェクトは琥珀、「その他」は薄い色
    val fill = when {
        r.other -> c.tx4
        r.series != null -> c.chartSeries[r.series]
        else -> c.accent
    }
    Column(Modifier.semantics(mergeDescendants = true) {}.testTag("breakdown-${r.key}"), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (r.series != null) Swatch(fill)
            Text(
                r.name,
                color = if (r.other) c.tx4 else c.tx2,
                fontFamily = MonoFont,
                fontSize = 12.5.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            Text(formatTokens(r.tokens), color = c.tx, fontFamily = MonoFont, fontSize = 12.5.sp, fontWeight = FontWeight.Medium)
            Text("${(r.share * 100).roundToInt()}%", color = c.tx4, fontFamily = MonoFont, fontSize = 12.sp, modifier = Modifier.width(40.dp), maxLines = 1)
        }
        Box(Modifier.fillMaxWidth().height(5.dp).clip(BAR_TRACK_SHAPE).background(c.bgChipDim)) {
            val share = if (r.share > 0f) maxOf(r.share, MIN_VISIBLE_SHARE) else 0f
            Box(Modifier.fillMaxWidth(share).fillMaxHeight().clip(BAR_TRACK_SHAPE).background(fill))
        }
        Text("${formatExact(r.sessions)} セッション", color = c.tx4, fontFamily = MonoFont, fontSize = 11.sp)
    }
}

/** 割合が小さくても棒が見えるだけの長さ (PC の 2%)。 */
private const val MIN_VISIBLE_SHARE = 0.02f

/** 日別のデータをカレンダーのように: 週の列 × 曜日の行 (日曜が先頭)、その日のトークンで濃く。タップしたマスの値を下に。 */
@Composable
fun Heatmap(report: UsageStats) {
    val c = Yhtye.colors
    val weeks = remember(report) { heatmapWeeks(report.days) }
    var selected by remember(report) { mutableStateOf<HeatCell?>(null) }
    val busiest = report.days.maxByOrNull { it.tokens.total }
    val label = if (busiest != null && busiest.tokens.total > 0) {
        "日別の利用ヒートマップ。最も多い日は ${busiest.date} (${formatTokens(busiest.tokens.total)} トークン)"
    } else {
        "日別の利用ヒートマップ。利用なし"
    }
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        BoxWithConstraints(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
            val count = weeks.size.coerceAtLeast(1)
            val cell: Dp = ((maxWidth - WEEKDAY_WIDTH - HEAT_GAP * count) / count).coerceIn(HEAT_MIN, HEAT_MAX)
            Row {
                Column(Modifier.width(WEEKDAY_WIDTH), verticalArrangement = Arrangement.spacedBy(HEAT_GAP)) {
                    WEEKDAYS.forEachIndexed { i, day ->
                        Box(Modifier.height(cell), contentAlignment = Alignment.CenterStart) {
                            if (i in LABELLED_WEEKDAYS) Text(day, color = c.tx4, fontSize = 10.sp, maxLines = 1)
                        }
                    }
                }
                HeatGrid(weeks, cell, selected, onSelect = { selected = if (selected == it) null else it }, label)
            }
        }
        selected?.let { cell ->
            Text(
                "${cell.date}: ${formatExact(cell.tokens)} トークン · ${cell.sessions} セッション",
                color = c.tx2,
                fontFamily = MonoFont,
                fontSize = 11.5.sp,
                modifier = Modifier.testTag("heat-detail"),
            )
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(3.dp)) {
            Text("少ない", color = c.tx4, fontSize = 10.5.sp, modifier = Modifier.padding(end = 4.dp))
            (0..HEAT_LEVELS).forEach { level -> Box(Modifier.size(10.dp).clip(SWATCH_SHAPE).background(c.heat[level])) }
            Text("多い", color = c.tx4, fontSize = 10.5.sp, modifier = Modifier.padding(start = 4.dp))
        }
    }
}

@Composable
private fun HeatGrid(weeks: List<List<HeatCell?>>, cell: Dp, selected: HeatCell?, onSelect: (HeatCell) -> Unit, label: String) {
    val c = Yhtye.colors
    val step = cell + HEAT_GAP
    Canvas(
        Modifier
            .width(step * weeks.size)
            .height(step * 7 - HEAT_GAP)
            .pointerInput(weeks, cell) {
                detectTapGestures { pos ->
                    val w = (pos.x / step.toPx()).toInt()
                    val d = (pos.y / step.toPx()).toInt()
                    weeks.getOrNull(w)?.getOrNull(d)?.let(onSelect)
                }
            }
            .semantics { contentDescription = label }
            .testTag("heatmap"),
    ) {
        val s = cell.toPx()
        val st = step.toPx()
        val radius = CornerRadius(3.dp.toPx())
        weeks.forEachIndexed { w, week ->
            week.forEachIndexed { d, day ->
                if (day == null) return@forEachIndexed
                val topLeft = Offset(w * st, d * st)
                drawRoundRect(c.heat[day.level.coerceIn(0, HEAT_LEVELS)], topLeft, Size(s, s), radius)
                if (day == selected) {
                    drawRoundRect(c.tx3, topLeft, Size(s, s), radius, style = Stroke(1.dp.toPx()))
                }
            }
        }
    }
}
