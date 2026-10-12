package io.github.tom039224.yhtye.shared.update

import io.github.tom039224.yhtye.shared.version.AppVersion
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class UpdateCheckerTest {
    private val current = AppVersion(0, 1, 0)

    private class FakeSource : ReleaseSource {
        var releases: List<GithubRelease> = emptyList()
        var failure: String? = null
        var gate: CompletableDeferred<Unit>? = null
        var calls = 0

        override suspend fun fetch(): List<GithubRelease> {
            calls++
            gate?.await()
            failure?.let { throw ReleaseFetchException(it) }
            return releases
        }
    }

    private val source = FakeSource()
    private val store = MemoryUpdateStore()

    private fun TestScope.checker(version: AppVersion = current) =
        UpdateChecker(source, version, store, backgroundScope, now = { START + currentTime })

    private fun newer(tag: String = "android-v0.1.1") = listOf(GithubRelease(tag, prerelease = true))

    // ---- 前面に来たとき ----

    @Test
    fun checksWhenTheAppComesToTheForegroundTheFirstTimeAndRemembersWhenAndWhatItFound() = runTest {
        source.releases = newer()
        val c = checker()
        assertEquals(UpdateState(), c.state.value)
        c.onForeground()
        runCurrent()
        val s = c.state.value
        assertEquals(AvailableUpdate(AppVersion(0, 1, 1), "android-v0.1.1"), s.available)
        assertEquals(START, s.checkedAt)
        assertFalse(s.checking)
        assertNull(s.failure)
        assertEquals(StoredUpdate(enabled = true, checkedAt = START, availableTag = "android-v0.1.1"), store.stored)
    }

    @Test
    fun doesNotCheckAgainWithinTwentyFourHours() = runTest {
        val c = checker()
        c.onForeground()
        runCurrent()
        advanceTimeBy(UPDATE_CHECK_INTERVAL_MS - 1)
        c.onForeground()
        runCurrent()
        assertEquals(1, source.calls)
        advanceTimeBy(1)
        c.onForeground()
        runCurrent()
        assertEquals(2, source.calls)
    }

    @Test
    fun theIntervalCountsFromTheLastCheckBeforeTheAppWasClosed() = runTest {
        store.stored = StoredUpdate(checkedAt = START - 3_600_000)
        checker().onForeground()
        runCurrent()
        assertEquals(0, source.calls)
        store.stored = StoredUpdate(checkedAt = START - UPDATE_CHECK_INTERVAL_MS)
        checker().onForeground()
        runCurrent()
        assertEquals(1, source.calls)
    }

    @Test
    fun checksRightAwayWhenTheClockWasSetBackBeforeTheLastCheck() = runTest {
        store.stored = StoredUpdate(checkedAt = START + 7 * 24 * 3_600_000L)
        checker().onForeground()
        runCurrent()
        assertEquals(1, source.calls)
    }

    @Test
    fun doesNotStartASecondCheckWhileOneIsRunning() = runTest {
        source.gate = CompletableDeferred()
        val c = checker()
        c.onForeground()
        runCurrent()
        assertTrue(c.state.value.checking)
        c.onForeground()
        c.checkNow()
        runCurrent()
        assertEquals(1, source.calls)
        source.gate?.complete(Unit)
        runCurrent()
        assertFalse(c.state.value.checking)
    }

    // ---- 結果 ----

    @Test
    fun saysNothingIsAvailableWhenThisAppIsTheNewest() = runTest {
        source.releases = listOf(GithubRelease("android-v0.1.0"), GithubRelease("v5.0.0"))
        val c = checker()
        c.checkNow()
        runCurrent()
        assertNull(c.state.value.available)
        assertEquals(START, c.state.value.checkedAt)
    }

    @Test
    fun aMarkFromAnEarlierCheckStaysUntilThisAppHasCaughtUp() = runTest {
        store.stored = StoredUpdate(checkedAt = START, availableTag = "android-v0.1.1")
        assertEquals("android-v0.1.1", checker().state.value.available?.tag)
        // その版を入れたあと (自分がそれ以上) は印を出さない
        assertNull(checker(AppVersion(0, 1, 1)).state.value.available)
        assertNull(checker(AppVersion(0, 2, 0)).state.value.available)
    }

    @Test
    fun aNewerReleaseThatAppearsLaterReplacesTheMark() = runTest {
        source.releases = newer("android-v0.1.1")
        val c = checker()
        c.checkNow()
        runCurrent()
        source.releases = newer("android-v0.1.2")
        c.checkNow()
        runCurrent()
        assertEquals("android-v0.1.2", c.state.value.available?.tag)
    }

    // ---- 失敗 ----

    @Test
    fun aFailureIsKeptAsAReasonAndDoesNotClearTheMarkOrTheTimeOfTheLastCheck() = runTest {
        source.releases = newer()
        val c = checker()
        c.checkNow()
        runCurrent()
        advanceTimeBy(1_000)
        source.failure = "HTTP 403 (rate limited?)"
        c.checkNow()
        runCurrent()
        val s = c.state.value
        assertEquals("HTTP 403 (rate limited?)", s.failure)
        assertEquals("android-v0.1.1", s.available?.tag)
        assertEquals(START, s.checkedAt)
        assertFalse(s.checking)
        // 成功すると理由は消える
        source.failure = null
        c.checkNow()
        runCurrent()
        assertNull(c.state.value.failure)
    }

    @Test
    fun anyKindOfFailureIsCaught() = runTest {
        val c = UpdateChecker({ error("boom") }, current, store, backgroundScope, now = { START + currentTime })
        c.checkNow()
        runCurrent()
        assertEquals("boom", c.state.value.failure)
        assertFalse(c.state.value.checking)
    }

    @Test
    fun afterAFailureTheAutomaticCheckWaitsAnHourButCheckNowDoesNot() = runTest {
        source.failure = "timed out"
        val c = checker()
        c.onForeground()
        runCurrent()
        assertEquals(1, source.calls)
        advanceTimeBy(UPDATE_RETRY_AFTER_FAILURE_MS - 1)
        c.onForeground()
        runCurrent()
        assertEquals(1, source.calls)
        c.checkNow()
        runCurrent()
        assertEquals(2, source.calls)
        advanceTimeBy(UPDATE_RETRY_AFTER_FAILURE_MS)
        c.onForeground()
        runCurrent()
        assertEquals(3, source.calls)
    }

    // ---- 設定の切り替え ----

    @Test
    fun whenTurnedOffItNeverTalksToGitHubAndShowsNoMark() = runTest {
        store.stored = StoredUpdate(checkedAt = START - UPDATE_CHECK_INTERVAL_MS, availableTag = "android-v0.1.1")
        val c = checker()
        c.setEnabled(false)
        runCurrent()
        assertFalse(c.state.value.enabled)
        assertNull(c.state.value.available)
        c.onForeground()
        c.checkNow()
        runCurrent()
        assertEquals(0, source.calls)
        assertEquals(false, store.stored.enabled)
        assertNull(store.stored.availableTag)
        // 次の起動でも切ったまま
        val again = checker()
        assertFalse(again.state.value.enabled)
        assertNull(again.state.value.available)
    }

    @Test
    fun turningItOffStopsACheckInProgress() = runTest {
        source.gate = CompletableDeferred()
        val c = checker()
        c.checkNow()
        runCurrent()
        assertTrue(c.state.value.checking)
        c.setEnabled(false)
        runCurrent()
        assertFalse(c.state.value.checking)
        source.gate?.complete(Unit)
        source.releases = newer()
        runCurrent()
        assertNull(c.state.value.available)
        assertNull(c.state.value.checkedAt)
    }

    @Test
    fun turningItBackOnChecksAtOnce() = runTest {
        source.releases = newer()
        store.stored = StoredUpdate(enabled = false)
        val c = checker()
        assertFalse(c.state.value.enabled)
        c.setEnabled(true)
        runCurrent()
        assertTrue(c.state.value.enabled)
        assertEquals("android-v0.1.1", c.state.value.available?.tag)
        assertTrue(store.stored.enabled)
    }

    private companion object {
        /** テストの時計の始まり (0 だと「最後の確認が無い」と見分けにくい)。 */
        const val START = 1_700_000_000_000L
    }
}
