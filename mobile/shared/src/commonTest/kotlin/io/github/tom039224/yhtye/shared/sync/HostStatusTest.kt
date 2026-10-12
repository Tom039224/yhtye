package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.UsageReport
import io.github.tom039224.yhtye.shared.api.generated.UsageWindow
import io.github.tom039224.yhtye.shared.api.generated.UsageWindowKind
import io.github.tom039224.yhtye.shared.client.ConnectionState
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** PC の `src/ui/hostStatus.test.ts` と `StatusBar.test.tsx` の計算部分の移植。 */
class HostStatusTest {
    private val open = ConnectionState.Connected(1)
    private val connecting = ConnectionState.Connecting(0)
    private val closed = ConnectionState.WaitingToReconnect(1, 1_000, null)

    @Test
    fun saysNowFor5SecondsOrLess() {
        assertEquals(listOf("now", "now", "now", "now"), listOf(0L, 1L, 4L, 5L).map(::formatPingAge))
    }

    @Test
    fun countsSecondsThenMinutesHoursAndDays() {
        assertEquals("6s", formatPingAge(6))
        assertEquals("59s", formatPingAge(59))
        assertEquals("1m", formatPingAge(60))
        assertEquals("59m", formatPingAge(3599))
        assertEquals("1h", formatPingAge(3600))
        assertEquals("23h", formatPingAge(86_399))
        assertEquals("1d", formatPingAge(86_400))
    }

    @Test
    fun pingAgeIsTheRoundedTimeSinceTheLastAnswerNeverNegative() {
        assertNull(pingAgeSeconds(10_000, null))
        assertEquals(6, pingAgeSeconds(10_000, 4_400))
        assertEquals(5, pingAgeSeconds(10_000, 4_600))
        assertEquals(0, pingAgeSeconds(10_000, 12_000))
    }

    @Test
    fun isGreenWhileTheCoreAnswers() {
        assertEquals(HostTone.Ok, hostTone(open, 0))
        assertEquals(HostTone.Ok, hostTone(open, 12))
    }

    @Test
    fun isAmberWhileConnectingOrBeforeTheFirstAnswer() {
        assertEquals(HostTone.Busy, hostTone(connecting, 0))
        assertEquals(HostTone.Busy, hostTone(ConnectionState.Idle, 0))
        assertEquals(HostTone.Busy, hostTone(open, null))
    }

    @Test
    fun isRedWhenDisconnectedOrAnOpenConnectionStoppedAnswering() {
        assertEquals(HostTone.Bad, hostTone(closed, 1))
        assertEquals(HostTone.Bad, hostTone(ConnectionState.Rejected(null), 1))
        assertEquals(HostTone.Bad, hostTone(open, 13))
    }

    @Test
    fun namesEachState() {
        assertEquals("接続中", hostToneLabel(HostTone.Ok, open))
        assertEquals("切断", hostToneLabel(HostTone.Bad, closed))
        assertEquals("応答なし", hostToneLabel(HostTone.Bad, open))
        assertEquals("接続を確認中", hostToneLabel(HostTone.Busy, connecting))
    }

    @Test
    fun namesThePlanAsTheClaudeSubscriptionItIs() {
        assertEquals("Claude Max", planLabel("max"))
        assertEquals("Claude Pro", planLabel("pro"))
        assertEquals("Claude Team", planLabel("Team"))
        assertEquals("Claude Enterprise", planLabel("Claude Enterprise"))
        assertEquals("Claude Max 5x", planLabel(" max 5x "))
    }

    @Test
    fun formatsTheTimeToReset() {
        val hour = 3_600_000L
        assertEquals("リセット済み", formatRemaining(0))
        assertEquals("リセット済み", formatRemaining(-5))
        assertEquals("12m", formatRemaining(12 * 60_000L))
        assertEquals("1h 0m", formatRemaining(hour))
        assertEquals("1d 2h", formatRemaining(26 * hour + 1))
    }

    @Test
    fun picksTheWindowOfAKind() {
        val week = UsageWindow(UsageWindowKind.Week, "Weekly · all models", 40.0)
        val report = UsageReport("max", listOf(UsageWindow(UsageWindowKind.Other, "Weekly · Opus", 10.0), week), 0)
        assertEquals(week, usageWindow(report, UsageWindowKind.Week))
        assertNull(usageWindow(report, UsageWindowKind.FiveHour))
        assertNull(usageWindow(null, UsageWindowKind.Week))
    }
}
