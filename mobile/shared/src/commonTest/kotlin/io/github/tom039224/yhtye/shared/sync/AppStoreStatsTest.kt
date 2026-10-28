package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.coroutines.EmptyCoroutineContext
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** `AppStore.refreshStats` (ダッシュボードの利用統計。PC の `get_usage_stats` の呼び方に合わせる)。 */
@OptIn(ExperimentalCoroutinesApi::class)
class AppStoreStatsTest {
    private val core = FakeCore(PROJECT, FULL_RUN.start)

    private fun TestScope.store(offset: Int = 540) =
        newStore(core, options = AppStore.Options(workContext = EmptyCoroutineContext, utcOffsetMin = { offset }))

    private fun statsCalls() = core.callsOf<ApiCommand.GetUsageStats>()

    @Test
    fun startsWithNoReportAndTheDefaultPeriod() = runTest {
        val s = store()
        val stats = s.state.value.stats
        assertNull(stats.report)
        assertNull(stats.error)
        assertFalse(stats.loading)
        assertEquals(DEFAULT_STATS_DAYS, stats.days)
        assertEquals(listOf(7, 30, 90, 365), STATS_PERIODS)
        assertTrue(statsCalls().isEmpty())
    }

    @Test
    fun readsThePeriodWithTheDevicesUtcOffset() = runTest {
        val s = store(offset = 540)
        s.refreshStats(7)
        runCurrent()

        assertEquals(listOf(ApiCommand.GetUsageStats(days = 7, tzOffsetMin = 540, refresh = null)), statsCalls())
        val stats = s.state.value.stats
        assertEquals(7, stats.days)
        assertFalse(stats.loading)
        assertNull(stats.error)
        val report = assertNotNull(stats.report)
        assertEquals(7, report.periodDays)
        assertEquals(540, report.tzOffsetMin)
        assertEquals(7, report.days.size)
        assertFalse(stats.isStale)
    }

    @Test
    fun usesTheDeviceOffsetAtThatMoment() = runTest {
        val offsets = mutableListOf<Long>()
        val s = newStore(
            core,
            options = AppStore.Options(workContext = EmptyCoroutineContext, utcOffsetMin = { at -> offsets += at; -300 }),
        )
        s.refreshStats(30)
        runCurrent()
        assertEquals(1, offsets.size)
        assertEquals(-300, statsCalls().single().tzOffsetMin)
    }

    @Test
    fun clampsAnOffsetBeyondWhatTheCoreAcceptsAndThePeriod() = runTest {
        val s = store(offset = 99_999)
        s.refreshStats(0)
        s.refreshStats(5_000)
        runCurrent()
        assertEquals(listOf<Long>(1, 366), statsCalls().map { it.days })
        assertTrue(statsCalls().all { it.tzOffsetMin == 24 * 60 })
        assertEquals(366, s.state.value.stats.days)
    }

    @Test
    fun asksTheCoreToForgetWhatItReadOnlyWhenToldTo() = runTest {
        val s = store()
        s.refreshStats(30, refresh = true)
        s.refreshStats(30)
        runCurrent()
        assertEquals(listOf(true, null), statsCalls().map { it.refresh })
    }

    @Test
    fun withNoArgumentReloadsTheSelectedPeriod() = runTest {
        val s = store()
        s.refreshStats(90)
        s.refreshStats()
        runCurrent()
        assertEquals(listOf<Long>(90, 90), statsCalls().map { it.days })
    }

    @Test
    fun keepsThePreviousReportWhileReloadingAndMarksItStaleForAnotherPeriod() = runTest {
        val s = store()
        s.refreshStats(30)
        runCurrent()
        val first = assertNotNull(s.state.value.stats.report)

        val gate = CompletableDeferred<Unit>()
        core.before = { if (it is ApiCommand.GetUsageStats) gate.await() }
        val job = backgroundScope.launch { s.refreshStats(7) }
        runCurrent()

        val loading = s.state.value.stats
        assertTrue(loading.loading)
        assertEquals(7, loading.days)
        assertEquals(first, loading.report)
        assertTrue(loading.isStale)

        gate.complete(Unit)
        runCurrent()
        job.join()
        val done = s.state.value.stats
        assertFalse(done.loading)
        assertEquals(7, assertNotNull(done.report).periodDays)
        assertFalse(done.isStale)
    }

    @Test
    fun aFailureKeepsTheReportAndShowsTheReasonUntilTheNextRead() = runTest {
        val s = store()
        s.refreshStats(30)
        runCurrent()
        val first = assertNotNull(s.state.value.stats.report)

        core.fail(ApiCommand.GetUsageStats::class, ApiErrorCode.InvalidArgument, "days must be 1-366")
        s.refreshStats(90)
        runCurrent()
        val failed = s.state.value.stats
        assertEquals("days must be 1-366", failed.error)
        assertFalse(failed.loading)
        assertEquals(first, failed.report)
        assertTrue(failed.isStale)
        // 画面のエラー一覧には出さない (統計の欄が出す)
        assertTrue(s.state.value.errors.isEmpty())

        core.failures.clear()
        s.refreshStats(90)
        runCurrent()
        val ok = s.state.value.stats
        assertNull(ok.error)
        assertEquals(90, assertNotNull(ok.report).periodDays)
    }

    @Test
    fun onlyTheLastOfSeveralReadsChangesTheState() = runTest {
        val s = store()
        val slow = CompletableDeferred<Unit>()
        var calls = 0
        core.before = { if (it is ApiCommand.GetUsageStats && calls++ == 0) slow.await() }

        val first = backgroundScope.launch { s.refreshStats(7) }
        runCurrent()
        val second = backgroundScope.launch { s.refreshStats(90) }
        runCurrent()
        assertEquals(90, s.state.value.stats.days)
        assertEquals(90, assertNotNull(s.state.value.stats.report).periodDays)
        assertFalse(s.state.value.stats.loading)

        // 遅れて届いた前の答えは捨てる
        slow.complete(Unit)
        runCurrent()
        first.join()
        second.join()
        val stats = s.state.value.stats
        assertEquals(90, stats.days)
        assertEquals(90, assertNotNull(stats.report).periodDays)
        assertFalse(stats.loading)
    }

    @Test
    fun aCancelledReadDoesNotLeaveTheStateLoading() = runTest {
        val s = store()
        val never = CompletableDeferred<Unit>()
        core.before = { if (it is ApiCommand.GetUsageStats) never.await() }
        val job = backgroundScope.launch { s.refreshStats(30) }
        runCurrent()
        assertTrue(s.state.value.stats.loading)

        job.cancel()
        runCurrent()
        assertFalse(s.state.value.stats.loading)
        assertNull(s.state.value.stats.error)
    }
}
