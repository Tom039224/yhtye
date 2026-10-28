package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.StatsDay
import io.github.tom039224.yhtye.shared.api.generated.StatsDayHarness

// 利用統計 (`UsageStats.days`: 期間の日ごと、古い順、使わなかった日も 1 日分) をグラフに出す形にする。

// ---- 日付 ----------------------------------------------------------------------------------------

/** `YYYY-MM-DD` の日番号 (1970-01-01 が 0)。実在しない日付・形が違うものは null。 */
fun epochDayOf(date: String): Long? {
    if (date.length != DATE_LENGTH || date[4] != '-' || date[7] != '-') return null
    val year = date.substring(0, 4).toIntOrNull() ?: return null
    val month = date.substring(5, 7).toIntOrNull() ?: return null
    val day = date.substring(8, 10).toIntOrNull() ?: return null
    if (month !in 1..12 || day !in 1..daysInMonth(year, month)) return null
    // Hinnant の days_from_civil
    val y = if (month <= 2) year - 1 else year
    val era = y.floorDiv(400)
    val yoe = y - era * 400
    val doy = (153 * (if (month > 2) month - 3 else month + 9) + 2) / 5 + day - 1
    val doe = yoe * 365 + yoe / 4 - yoe / 100 + doy
    return era.toLong() * 146_097 + doe - 719_468
}

/** 日番号 [epochDay] の曜日: 0 = 日曜 … 6 = 土曜 (1970-01-01 は木曜)。 */
fun weekdayOf(epochDay: Long): Int = (epochDay + 4).mod(7L).toInt()

private fun daysInMonth(year: Int, month: Int): Int = when (month) {
    2 -> if (year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)) 29 else 28
    4, 6, 9, 11 -> 30
    else -> 31
}

private const val DATE_LENGTH = 10

// ---- ヒートマップ ----------------------------------------------------------------------------------

/** ヒートマップの濃さの段数 (1..この数。使わなかった日は 0)。 */
const val HEAT_LEVELS: Int = 4

/** ヒートマップの 1 日。 */
data class HeatCell(
    /** `YYYY-MM-DD`。 */
    val date: String,
    val tokens: Long,
    val sessions: Long,
    /** 0 = 使わなかった日、1..[HEAT_LEVELS] = 期間で一番多い日に対する割合 (多いほど濃い)。 */
    val level: Int,
)

/** [tokens] の濃さ ([max] = 期間で一番多い日。使った日は最低でも 1)。 */
fun heatLevel(tokens: Long, max: Long): Int {
    if (tokens <= 0 || max <= 0) return 0
    return ((tokens * HEAT_LEVELS + max - 1) / max).toInt().coerceIn(1, HEAT_LEVELS)
}

/**
 * [days] (古い順) を週の列に並べる (GitHub の貢献グラフの形): 各列は 7 マスで、上から [firstWeekday] (0 = 日曜 … 6 = 土曜)
 * の曜日で始まる。期間の最初の週の前と最後の週の後ろの、期間の外のマスは null。日付が読めなければ最初の日を週の頭に置く。
 */
fun heatmapWeeks(days: List<StatsDay>, firstWeekday: Int = 0): List<List<HeatCell?>> {
    if (days.isEmpty()) return emptyList()
    val max = days.maxOf { it.tokens.total }
    val offset = epochDayOf(days.first().date)?.let { (weekdayOf(it) - firstWeekday).mod(DAYS_PER_WEEK) } ?: 0
    val cells: List<HeatCell?> = List(offset) { null } + days.map { HeatCell(it.date, it.tokens.total, it.sessions, heatLevel(it.tokens.total, max)) }
    return cells.chunked(DAYS_PER_WEEK).map { week -> week + List(DAYS_PER_WEEK - week.size) { null } }
}

private const val DAYS_PER_WEEK = 7

// ---- 日のまとまり (棒グラフ) -------------------------------------------------------------------------

/** 連続する [days] 日をまとめた 1 本の棒。 */
data class DayBucket(
    /** まとまりの最初の日と最後の日 (`YYYY-MM-DD`。1 日だけなら同じ)。 */
    val from: String,
    val to: String,
    /** まとめた日数 (最初のまとまりだけ、割り切れないと [bucketDays] の `size` より少ない)。 */
    val days: Int,
    val tokens: Long,
    /** 日ごとのセッション数の和 (何日かにまたがるセッションは日ごとに数える。期間全体の数は `UsageStats.totals.sessions`)。 */
    val sessions: Long,
    /** ハーネスごとの内訳 (トークンが多い順)。 */
    val harnesses: List<StatsDayHarness>,
)

/**
 * [days] (古い順) を [size] 日ずつのまとまりにする。今日を含む側から区切るので、割り切れないときに少なくなるのは
 * 一番古いまとまり (今日の棒が低く見えない)。[size] が 1 以下なら 1 日ずつ。
 */
fun bucketDays(days: List<StatsDay>, size: Int): List<DayBucket> {
    val width = size.coerceAtLeast(1)
    val first = days.size % width
    val groups = buildList {
        if (first > 0) add(days.subList(0, first))
        addAll(days.subList(first, days.size).chunked(width))
    }
    return groups.map { group ->
        val byHarness = LinkedHashMap<String, StatsDayHarness>()
        for (h in group.flatMap { it.harnesses }) {
            val sum = byHarness[h.harness]
            byHarness[h.harness] = if (sum == null) h else sum.copy(tokens = sum.tokens + h.tokens, sessions = sum.sessions + h.sessions)
        }
        DayBucket(
            from = group.first().date,
            to = group.last().date,
            days = group.size,
            tokens = group.sumOf { it.tokens.total },
            sessions = group.sumOf { it.sessions },
            harnesses = byHarness.values.sortedByDescending { it.tokens },
        )
    }
}

/** 期間 [periodDays] 日の棒グラフの 1 本の日数: 1 か月までは 1 日、3 か月までは 3 日、それより長ければ 1 週間 (棒を 60 本以内に)。 */
fun barBucketSize(periodDays: Int): Int = when {
    periodDays <= 31 -> 1
    periodDays <= 92 -> 3
    else -> 7
}
