package io.github.tom039224.yhtye.ui.dashboard

import io.github.tom039224.yhtye.shared.api.generated.StatsBucket
import io.github.tom039224.yhtye.shared.api.generated.StatsDayHarness
import io.github.tom039224.yhtye.shared.api.generated.StatsProject
import io.github.tom039224.yhtye.shared.api.generated.StatsSource
import io.github.tom039224.yhtye.shared.api.generated.StatsTokens
import io.github.tom039224.yhtye.shared.api.generated.StatsTotals
import io.github.tom039224.yhtye.shared.sync.DayBucket
import io.github.tom039224.yhtye.ui.cannedUsageStats
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 利用統計の数の書き方・色の順・内訳・目盛り (PC の `usageStats.test.ts` と同じ事例)。 */
class StatsModelTest {
    private fun tokens(total: Long) = StatsTokens(total, 0, 0, 0, total)

    @Test
    fun `tokens shrink to three significant figures with K M B`() {
        assertEquals("0", formatTokens(0))
        assertEquals("999", formatTokens(999))
        assertEquals("1K", formatTokens(1_000))
        assertEquals("1.2K", formatTokens(1_234))
        assertEquals("12.3K", formatTokens(12_345))
        assertEquals("123K", formatTokens(123_456))
        assertEquals("1M", formatTokens(999_999))
        assertEquals("1.5M", formatTokens(1_500_000))
        assertEquals("2.5B", formatTokens(2_500_000_000))
        assertEquals("0", formatTokens(-5))
        assertEquals("1,234,567", formatExact(1_234_567))
    }

    @Test
    fun `the scale tops at 1, 2 or 5 times a power of ten`() {
        assertEquals(1, niceCeil(0))
        assertEquals(1, niceCeil(1))
        assertEquals(2, niceCeil(2))
        assertEquals(5, niceCeil(3))
        assertEquals(10, niceCeil(7))
        assertEquals(20_000, niceCeil(11_100))
        assertEquals(50_000, niceCeil(40_001))
    }

    @Test
    fun `harnesses take colors in the order the core looked for them, then the others alphabetically`() {
        val base = cannedUsageStats(7, 0)
        val report = base.copy(
            days = base.days.mapIndexed { i, d -> if (i == 0) d.copy(harnesses = listOf(StatsDayHarness("zed", 1, 1))) else d },
            byHarness = base.byHarness + StatsBucket("amp", tokens(1), 1, other = false) + StatsBucket("other", tokens(1), 1, other = true),
        )
        val order = harnessOrder(report)
        assertEquals(listOf("claude-code", "codex", "opencode", "amp", "zed"), order)
        assertEquals(0, seriesIndex(order, "claude-code"))
        assertEquals(4, seriesIndex(order, "zed"))
        // 知らないハーネスは末尾の次、色の数を超えると巡る
        assertEquals(5, seriesIndex(order, "nope"))
        assertEquals(0, seriesIndex(order + "f" + "g", "g"))
        assertEquals("Claude Code", harnessLabel("claude-code"))
        assertEquals("amp", harnessLabel("amp"))
    }

    @Test
    fun `bars stack the harnesses in the color order and leave out the unused ones`() {
        val bucket = DayBucket(
            "2026-10-18",
            "2026-10-20",
            3,
            30,
            3,
            listOf(StatsDayHarness("codex", 20, 1), StatsDayHarness("claude-code", 10, 2), StatsDayHarness("opencode", 0, 0)),
        )
        val bars = barSeries(listOf(bucket), listOf("claude-code", "codex", "opencode"))
        assertEquals(listOf("claude-code" to 10L, "codex" to 20L), bars.single().segments)
        assertEquals("10/18–10/20", bucketLabel(bucket))
        assertEquals("10/20", bucketLabel(bucket.copy(from = "2026-10-20")))
        assertEquals("1/5", shortDate("2026-01-05"))
    }

    @Test
    fun `breakdown rows carry their share, the other entry and the harness color`() {
        val rows = breakdownRows(
            listOf(StatsBucket("codex", tokens(75), 3, false), StatsBucket("x", tokens(25), 1, true)),
            ::harnessLabel,
            listOf("claude-code", "codex"),
        )
        assertEquals(listOf("Codex", OTHER_LABEL), rows.map { it.name })
        assertEquals(listOf(0.75f, 0.25f), rows.map { it.share })
        assertEquals(1, rows[0].series)
        assertNull(rows[1].series)
        assertTrue(rows[1].other)
        assertEquals(0f, breakdownRows(listOf(StatsBucket("m", tokens(0), 0, false))).single().share)

        val projects = projectRows(listOf(StatsProject("repo", "a", tokens(1), 1, false), StatsProject("repo", null, tokens(1), 1, false)))
        assertEquals(2, projects.map { it.key }.toSet().size)
    }

    @Test
    fun `no logs and no usage are told apart`() {
        val report = cannedUsageStats(7, 0)
        assertFalse(noLogsFound(report))
        assertFalse(noUsage(report))
        assertTrue(noLogsFound(report.copy(sources = listOf(StatsSource("claude-code", false, 0)))))
        val zero = StatsTokens(0, 0, 0, 0, 0)
        assertTrue(noUsage(report.copy(totals = StatsTotals(zero, 0))))
        assertFalse(noUsage(report.copy(totals = StatsTotals(zero, 1))))
    }
}
