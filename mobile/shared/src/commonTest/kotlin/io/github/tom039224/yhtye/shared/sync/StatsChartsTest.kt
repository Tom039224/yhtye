package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.StatsDay
import io.github.tom039224.yhtye.shared.api.generated.StatsDayHarness
import io.github.tom039224.yhtye.shared.api.generated.StatsTokens
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 利用統計のグラフ用の整形 (日付・ヒートマップ・日のまとまり)。 */
class StatsChartsTest {
    private fun day(date: String, total: Long = 0, sessions: Long = if (total > 0) 1 else 0, vararg harnesses: Pair<String, Long>) =
        StatsDay(
            date = date,
            tokens = StatsTokens(input = total, output = 0, cacheRead = 0, cacheCreation = 0, total = total),
            sessions = sessions,
            harnesses = harnesses.map { (h, t) -> StatsDayHarness(h, t, 1) },
        )

    /** 2026-10-14 (水) から [n] 日、トークンは [tokens] の順 (足りない分は 0)。 */
    private fun week(n: Int, vararg tokens: Long): List<StatsDay> =
        (0 until n).map { day("2026-10-${14 + it}", tokens.getOrElse(it) { 0 }) }

    // ---- 日付 ----

    @Test
    fun parsesDatesToEpochDays() {
        assertEquals(0L, epochDayOf("1970-01-01"))
        assertEquals(20_746L, epochDayOf("2026-10-20")) // FakeCore の最後の日と同じ
        assertEquals(19_782L, epochDayOf("2024-02-29"))
        assertEquals(-1L, epochDayOf("1969-12-31"))
    }

    @Test
    fun rejectsWhatIsNotADate() {
        listOf("", "2026-10-2", "2026/10/20", "2026-13-01", "2026-00-10", "2026-02-29", "2026-04-31", "2026-10-00", "abcd-ef-gh", "2026-10-200")
            .forEach { assertNull(epochDayOf(it), it) }
        assertEquals(19_782L, epochDayOf("2024-02-29"))
    }

    @Test
    fun weekdayCountsFromSunday() {
        assertEquals(4, weekdayOf(epochDayOf("1970-01-01")!!)) // 木
        assertEquals(2, weekdayOf(epochDayOf("2026-10-20")!!)) // 火
        assertEquals(0, weekdayOf(epochDayOf("2026-10-18")!!)) // 日
        assertEquals(6, weekdayOf(epochDayOf("2026-10-17")!!)) // 土
        assertEquals(3, weekdayOf(epochDayOf("1969-12-31")!!)) // 水 (エポックの前でも)
    }

    // ---- ヒートマップ ----

    @Test
    fun heatLevelsAreRelativeToTheBusiestDay() {
        assertEquals(0, heatLevel(0, 100))
        assertEquals(0, heatLevel(5, 0))
        assertEquals(1, heatLevel(1, 100))
        assertEquals(1, heatLevel(25, 100))
        assertEquals(2, heatLevel(26, 100))
        assertEquals(3, heatLevel(75, 100))
        assertEquals(4, heatLevel(76, 100))
        assertEquals(HEAT_LEVELS, heatLevel(100, 100))
    }

    @Test
    fun layoutsDaysInWeekColumnsStartingOnSunday() {
        // 10-14 (水) から 12 日 = 水木金土 | 日月火水木金土 | 日
        val weeks = heatmapWeeks(week(12, 100, 0, 50, 0, 0, 0, 0, 0, 0, 0, 0, 25))
        assertEquals(listOf(7, 7, 7), weeks.map { it.size })
        assertEquals(listOf(null, null, null, "2026-10-14", "2026-10-15", "2026-10-16", "2026-10-17"), weeks[0].map { it?.date })
        assertEquals(listOf("2026-10-18", "2026-10-19", "2026-10-20", "2026-10-21", "2026-10-22", "2026-10-23", "2026-10-24"), weeks[1].map { it?.date })
        assertEquals(listOf("2026-10-25", null, null, null, null, null, null), weeks[2].map { it?.date })
        assertEquals(listOf(4, 0, 2), listOf(weeks[0][3]!!.level, weeks[0][4]!!.level, weeks[0][5]!!.level))
        assertEquals(1, weeks[2][0]!!.level)
        assertEquals(100, weeks[0][3]!!.tokens)
    }

    @Test
    fun theWeekCanStartOnMonday() {
        val weeks = heatmapWeeks(week(3), firstWeekday = 1)
        assertEquals(listOf(null, null, "2026-10-14", "2026-10-15", "2026-10-16", null, null), weeks.single().map { it?.date })
        // 月曜から始まる期間は前にパディングしない
        assertEquals("2026-10-19", heatmapWeeks(listOf(day("2026-10-19")), firstWeekday = 1).single().first()?.date)
    }

    @Test
    fun anEmptyPeriodHasNoWeeksAndAQuietOneNoColour() {
        assertTrue(heatmapWeeks(emptyList()).isEmpty())
        assertTrue(heatmapWeeks(week(7)).flatten().filterNotNull().all { it.level == 0 })
    }

    @Test
    fun aYearFitsInFiftyThreeColumns() {
        val days = (0 until 365).map { day(dateString(20_746L - 364 + it), it.toLong()) }
        val weeks = heatmapWeeks(days)
        assertTrue(weeks.size in 52..54, "${weeks.size} columns")
        assertEquals(365, weeks.flatten().filterNotNull().size)
        assertTrue(weeks.all { it.size == 7 })
    }

    @Test
    fun anUnreadableFirstDateStartsTheFirstColumnAtTheFirstDay() {
        val weeks = heatmapWeeks(listOf(day("??", 5), day("??", 5)))
        assertEquals(listOf("??", "??"), weeks.single().filterNotNull().map { it.date })
        assertEquals("??", weeks.single().first()?.date)
    }

    // ---- 日のまとまり ----

    @Test
    fun oneDayBucketsAreTheDaysThemselves() {
        val days = week(3, 10, 0, 30)
        val buckets = bucketDays(days, 1)
        assertEquals(listOf(10L, 0L, 30L), buckets.map { it.tokens })
        assertEquals(listOf("2026-10-14", "2026-10-15", "2026-10-16"), buckets.map { it.from })
        assertEquals(buckets.map { it.from }, buckets.map { it.to })
        assertEquals(listOf(1, 1, 1), buckets.map { it.days })
        assertEquals(buckets, bucketDays(days, 0))
    }

    @Test
    fun bucketsCountFromTodaySoOnlyTheOldestOneIsShort() {
        val days = week(7, 1, 2, 3, 4, 5, 6, 7)
        val buckets = bucketDays(days, 3)
        assertEquals(listOf(1, 3, 3), buckets.map { it.days })
        assertEquals(listOf(1L, 2 + 3 + 4L, 5 + 6 + 7L), buckets.map { it.tokens })
        assertEquals(listOf("2026-10-14", "2026-10-15", "2026-10-18"), buckets.map { it.from })
        assertEquals(listOf("2026-10-14", "2026-10-17", "2026-10-20"), buckets.map { it.to })
        assertEquals(days.sumOf { it.tokens.total }, buckets.sumOf { it.tokens })
    }

    @Test
    fun bucketsSumSessionsAndHarnessesBiggestFirst() {
        val days = listOf(
            day("2026-10-19", 100, sessions = 2, "codex" to 30, "claude-code" to 70),
            day("2026-10-20", 90, sessions = 1, "codex" to 90),
        )
        val bucket = bucketDays(days, 7).single()
        assertEquals(190L, bucket.tokens)
        assertEquals(3L, bucket.sessions)
        assertEquals(listOf("codex" to 120L, "claude-code" to 70L), bucket.harnesses.map { it.harness to it.tokens })
        assertEquals(listOf(2L, 1L), bucket.harnesses.map { it.sessions })
    }

    @Test
    fun noDaysNoBuckets() {
        assertTrue(bucketDays(emptyList(), 3).isEmpty())
    }

    @Test
    fun barsPerPeriodStayUnderSixty() {
        assertEquals(1, barBucketSize(7))
        assertEquals(1, barBucketSize(30))
        assertEquals(3, barBucketSize(90))
        assertEquals(7, barBucketSize(365))
        for (period in STATS_PERIODS + MAX_STATS_DAYS) {
            val days = (0 until period).map { day(dateString(20_746L - period + 1 + it)) }
            assertTrue(bucketDays(days, barBucketSize(period)).size <= 60, "$period days")
        }
    }

    /** [epochDay] の `YYYY-MM-DD` (Hinnant の civil_from_days。`epochDayOf` と突き合わせる)。 */
    private fun dateString(epochDay: Long): String {
        val z = epochDay + 719_468
        val era = z.floorDiv(146_097L)
        val doe = z - era * 146_097
        val yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365
        val doy = doe - (365 * yoe + yoe / 4 - yoe / 100)
        val mp = (5 * doy + 2) / 153
        val d = doy - (153 * mp + 2) / 5 + 1
        val m = if (mp < 10) mp + 3 else mp - 9
        val y = yoe + era * 400 + if (m <= 2) 1 else 0
        return "${y.toString().padStart(4, '0')}-${m.toString().padStart(2, '0')}-${d.toString().padStart(2, '0')}"
    }

    @Test
    fun dateRoundTripsThroughEpochDays() {
        for (d in listOf(-1000L, -1L, 0L, 365L, 11_016L, 19_782L, 20_746L, 100_000L)) {
            assertEquals(d, epochDayOf(dateString(d)), "day $d")
        }
    }
}
