package io.github.tom039224.yhtye.ui.dashboard

import io.github.tom039224.yhtye.shared.api.generated.StatsBucket
import io.github.tom039224.yhtye.shared.api.generated.StatsProject
import io.github.tom039224.yhtye.shared.api.generated.UsageStats
import io.github.tom039224.yhtye.shared.sync.DayBucket
import java.util.Locale
import kotlin.math.floor
import kotlin.math.log10
import kotlin.math.pow
import kotlin.math.roundToLong

// ダッシュボードの利用統計に描くもの (PC の `src/ui/usageStats.ts`): 数の書き方、ハーネスの色の順、内訳の行、棒グラフの
// 目盛り。日のまとまりとヒートマップのマスは共有層 (`StatsCharts.kt`)。純粋な関数なので画面なしで確かめられる。

/** ダッシュボードが出す期間 (日数。PC と同じ)。 */
val PERIODS: List<Int> = listOf(7, 30, 90)

/** ハーネスの色の数 (`chartSeries`)。それより多いハーネスは色を分け合う。 */
const val SERIES_COUNT: Int = 6

private val HARNESS_LABEL = mapOf("claude-code" to "Claude Code", "codex" to "Codex", "opencode" to "OpenCode")

/** ハーネスの ID の見せ方。 */
fun harnessLabel(id: String): String = HARNESS_LABEL[id] ?: id

private const val THOUSAND = 1_000.0
private const val UNITS = "KMB"
private const val ONE_DIGIT_BELOW = 100.0

/** `1234` → `1.2K`、`1_500_000` → `1.5M`: 有効数字 3 桁まで。 */
fun formatTokens(n: Long): String {
    val value = n.coerceAtLeast(0)
    if (value < THOUSAND) return value.toString()
    var unit = when {
        value >= 1e9 -> 3
        value >= 1e6 -> 2
        else -> 1
    }
    var scaled = value / THOUSAND.pow(unit)
    // 999_999 は「1000K」ではなく「1M」
    if (unit < 3 && (scaled * 10).roundToLong() / 10.0 >= THOUSAND) {
        unit += 1
        scaled = value / THOUSAND.pow(unit)
    }
    val digits = if (scaled < ONE_DIGIT_BELOW) {
        val tenths = (scaled * 10).roundToLong()
        if (tenths % 10 == 0L) (tenths / 10).toString() else "${tenths / 10}.${tenths % 10}"
    } else {
        scaled.roundToLong().toString()
    }
    return "$digits${UNITS[unit - 1]}"
}

/** 桁区切りの正確な数 (詳しい値)。 */
fun formatExact(n: Long): String = String.format(Locale.US, "%,d", n)

/**
 * 色を割り当てるハーネスの順: コアが探したもの (`sources` の順)、続いてデータにだけ出てくるものを名前順。同じ番号が
 * どのグラフでも同じ色。
 */
fun harnessOrder(report: UsageStats): List<String> {
    val known = report.sources.map { it.harness }
    val extra = (report.days.flatMap { day -> day.harnesses.map { it.harness } } + report.byHarness.filter { !it.other }.map { it.name })
        .filter { it !in known }
        .toSortedSet()
    return known + extra
}

/** ハーネスの色の番号 (`chartSeries` の添字)。 */
fun seriesIndex(order: List<String>, harness: String): Int {
    val at = order.indexOf(harness)
    return (if (at < 0) order.size else at) % SERIES_COUNT
}

/** 棒グラフの 1 本: まとめた日と、[order] の順に積むハーネスごとのトークン (無いハーネスは除く)。 */
data class BarSeries(val bucket: DayBucket, val segments: List<Pair<String, Long>>)

/** 日のまとまりを、ハーネスを [order] の順に積む棒にする。 */
fun barSeries(buckets: List<DayBucket>, order: List<String>): List<BarSeries> = buckets.map { b ->
    val byHarness = b.harnesses.associate { it.harness to it.tokens }
    val segments = (order + byHarness.keys.filter { it !in order }.sorted()).mapNotNull { h ->
        byHarness[h]?.takeIf { it > 0 }?.let { h to it }
    }
    BarSeries(b, segments)
}

/** [max] を 1・2・5 × 10^k に切り上げる: 棒グラフの目盛りの上端 (使用が無ければ 1)。 */
fun niceCeil(max: Long): Long {
    if (max <= 0) return 1
    val magnitude = 10.0.pow(floor(log10(max.toDouble())))
    val f = max / magnitude
    val step = when {
        f <= 1 -> 1
        f <= 2 -> 2
        f <= 5 -> 5
        else -> 10
    }
    return (step * magnitude).roundToLong()
}

/** 内訳の 1 行。 */
data class BreakdownRow(
    val key: String,
    val name: String,
    val tokens: Long,
    val sessions: Long,
    /** 一覧の合計に対する割合 (0..1)。 */
    val share: Float,
    /** コアが長い一覧から切った分の和 (「その他」)。 */
    val other: Boolean,
    /** ハーネスの行ならその色の番号。 */
    val series: Int? = null,
)

/** 一覧 (コアがトークンの多い順に並べたもの) の行: それぞれ一覧の合計に対する割合付き。 */
fun breakdownRows(buckets: List<StatsBucket>, label: (String) -> String = { it }, order: List<String>? = null): List<BreakdownRow> {
    val total = buckets.sumOf { it.tokens.total }
    return buckets.mapIndexed { i, b ->
        BreakdownRow(
            key = "$i:${b.name}",
            name = if (b.other) OTHER_LABEL else label(b.name),
            tokens = b.tokens.total,
            sessions = b.sessions,
            share = if (total > 0) (b.tokens.total.toDouble() / total).toFloat() else 0f,
            other = b.other,
            series = if (order != null && !b.other) seriesIndex(order, b.name) else null,
        )
    }
}

/** プロジェクト別の行 (同じ名前のプロジェクトがあっても行を分ける)。 */
fun projectRows(projects: List<StatsProject>): List<BreakdownRow> =
    breakdownRows(projects.map { StatsBucket(it.name, it.tokens, it.sessions, it.other) }).mapIndexed { i, row ->
        row.copy(key = "$i:${projects[i].project.orEmpty()}:${projects[i].name}")
    }

const val OTHER_LABEL = "その他"

/** どのハーネスのログもホストに無い。 */
fun noLogsFound(report: UsageStats): Boolean = report.sources.none { it.found }

/** ログはあるが期間に使っていない。 */
fun noUsage(report: UsageStats): Boolean = report.totals.tokens.total == 0L && report.totals.sessions == 0L

/** `2026-10-20` → `10/20`。 */
fun shortDate(date: String): String {
    val parts = date.split("-")
    if (parts.size != 3) return date
    val m = parts[1].toIntOrNull() ?: return date
    val d = parts[2].toIntOrNull() ?: return date
    return "$m/$d"
}

/** 棒 1 本の期間: 1 日なら `10/20`、まとめていれば `10/18–10/20`。 */
fun bucketLabel(bucket: DayBucket): String =
    if (bucket.from == bucket.to) shortDate(bucket.from) else "${shortDate(bucket.from)}–${shortDate(bucket.to)}"
