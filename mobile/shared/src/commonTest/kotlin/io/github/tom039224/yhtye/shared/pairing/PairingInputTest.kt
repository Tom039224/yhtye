package io.github.tom039224.yhtye.shared.pairing

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class PairingInputTest {
    private fun reason(result: Result<PairingTarget>): InvalidPairingInput? =
        (result.exceptionOrNull() as? InvalidPairingInputException)?.reason

    @Test
    fun theQrCodeOfThePcIsRead() {
        // core-design.md §18.5 の例と同じ形 (PC の pairing_uri のテストと同じ)
        val uri = "yhtye://pair?v=1&host=pc.tail1234.ts.net&ip=100.89.117.15&port=1423&code=ABCD2345&sid=sid1"
        val target = parsePairingUri(uri).getOrThrow()
        assertEquals(listOf("pc.tail1234.ts.net", "100.89.117.15"), target.hosts)
        assertEquals(1423, target.port)
        assertEquals("ABCD2345", target.code)
        assertEquals("sid1", target.serverId)
    }

    @Test
    fun aQrCodeWithoutMagicDnsHasOnlyTheIp() {
        val target = parsePairingUri("yhtye://pair?v=1&ip=100.64.0.1&port=9000&code=ABCD2345&sid=s").getOrThrow()
        assertEquals(listOf("100.64.0.1"), target.hosts)
        assertEquals(9000, target.port)
    }

    @Test
    fun valuesArePercentDecoded() {
        val uri = "yhtye://pair?v=1&host=pc.tail1234.ts.net&ip=fd7a%3A115c%3Aa1e0%3A%3A1&port=1423&code=abcd%2D2345&sid=s"
        val target = parsePairingUri(uri).getOrThrow()
        assertEquals(listOf("pc.tail1234.ts.net", "fd7a:115c:a1e0::1"), target.hosts)
        assertEquals("ABCD2345", target.code)
    }

    @Test
    fun otherQrCodesAndBrokenOnesAreRefused() {
        assertEquals(InvalidPairingInput.NotPairingUri, reason(parsePairingUri("https://example.com/")))
        assertEquals(InvalidPairingInput.NotPairingUri, reason(parsePairingUri("yhtye://pairing?v=1")))
        assertEquals(InvalidPairingInput.NotPairingUri, reason(parsePairingUri("yhtye://pair?ip=100.64.0.1")))
        assertEquals(
            InvalidPairingInput.UnsupportedVersion("2"),
            reason(parsePairingUri("yhtye://pair?v=2&ip=100.64.0.1&port=1&code=ABCD2345")),
        )
        assertEquals(
            InvalidPairingInput.MissingHost,
            reason(parsePairingUri("yhtye://pair?v=1&port=1423&code=ABCD2345&sid=s")),
        )
        assertEquals(
            InvalidPairingInput.InvalidPort,
            reason(parsePairingUri("yhtye://pair?v=1&ip=100.64.0.1&port=0&code=ABCD2345")),
        )
        assertEquals(
            InvalidPairingInput.MissingCode,
            reason(parsePairingUri("yhtye://pair?v=1&ip=100.64.0.1&port=1423")),
        )
    }

    @Test
    fun hostsOutsideTailscaleAreRefused() {
        assertEquals(
            InvalidPairingInput.NotTailscaleHost("192.168.0.10"),
            reason(parsePairingUri("yhtye://pair?v=1&ip=192.168.0.10&port=1423&code=ABCD2345&sid=s")),
        )
        assertEquals(
            InvalidPairingInput.NotTailscaleHost("evil.example.com"),
            reason(parseManualEntry("evil.example.com", "", "ABCD2345")),
        )
    }

    @Test
    fun manualEntryAcceptsHowPeopleTypeTheCode() {
        val target = parseManualEntry(" PC.tail1234.ts.net. ", "", "abcd-2345").getOrThrow()
        assertEquals(listOf("pc.tail1234.ts.net"), target.hosts)
        assertEquals(DEFAULT_PORT, target.port)
        assertEquals("ABCD2345", target.code)
        assertNull(target.serverId)
        assertEquals("ABCD2345", parseManualEntry("100.89.117.15", "1423", "ab cd 23 45").getOrThrow().code)
        assertEquals(listOf("fd7a:115c:a1e0::5"), parseManualEntry("[fd7a:115c:a1e0::5]", "1423", "ABCD2345").getOrThrow().hosts)
    }

    @Test
    fun fullWidthInputFromAJapaneseImeIsAccepted() {
        val target = parseManualEntry("１００．８９．１１７．１５", "１４２３", "ａｂｃｄ－２３４５").getOrThrow()
        assertEquals(listOf("100.89.117.15"), target.hosts)
        assertEquals(1423, target.port)
        assertEquals("ABCD2345", target.code)
        assertEquals("ABCD2345", normalizePairingCode("ABCD\u30002345"))
    }

    @Test
    fun manualEntryRefusesBadValues() {
        assertEquals(InvalidPairingInput.MissingHost, reason(parseManualEntry(" ", "", "ABCD2345")))
        assertEquals(InvalidPairingInput.InvalidPort, reason(parseManualEntry("100.64.0.1", "70000", "ABCD2345")))
        assertEquals(InvalidPairingInput.InvalidPort, reason(parseManualEntry("100.64.0.1", "abc", "ABCD2345")))
        assertEquals(InvalidPairingInput.InvalidPort, reason(parseManualEntry("100.64.0.1", "+80", "ABCD2345")))
        assertEquals(InvalidPairingInput.MissingCode, reason(parseManualEntry("100.64.0.1", "", "")))
        // 7 文字、9 文字、使わない文字 (0, O, 1, I, L)
        for (code in listOf("ABCD234", "ABCD23456", "ABCD2340", "ABCD234O", "ABCD2341", "abcd234l")) {
            assertEquals(InvalidPairingInput.InvalidCode, reason(parseManualEntry("100.64.0.1", "", code)), code)
        }
    }

    @Test
    fun tailscaleAddressesAreRecognized() {
        for (host in listOf("100.64.0.0", "100.127.255.255", "100.89.117.15", "fd7a:115c:a1e0::1", "fd7a:115c:a1e0:ab12:4843:cd96:625e:7f0f", "[FD7A:115C:A1E0::1]", "pc.tail1234.ts.net", "a.b.ts.net.")) {
            assertTrue(TailscaleHosts.isTailscaleHost(host), host)
        }
        for (host in listOf("100.63.255.255", "100.128.0.0", "10.0.0.1", "127.0.0.1", "100.64.0", "100.64.0.256", "fd7a:115c:a1e1::1", "::1", "fd7a::115c::1", "ts.net", "tail1234.ts.net", "pc.tail1234.ts.net.evil.com", "-x.tail.ts.net", "", "localhost")) {
            assertFalse(TailscaleHosts.isTailscaleHost(host), host)
        }
        assertEquals("[fd7a:115c:a1e0::1]", TailscaleHosts.urlHost("fd7a:115c:a1e0::1"))
        assertEquals("100.64.0.1", TailscaleHosts.urlHost("100.64.0.1"))
    }

    @Test
    fun deviceNamesFollowThePcRules() {
        assertEquals("Pixel 9", normalizeDeviceName("  Pixel 9\n"))
        assertNull(normalizeDeviceName(" \t"))
        assertEquals("x".repeat(64), normalizeDeviceName("x".repeat(64)))
        assertNull(normalizeDeviceName("x".repeat(65)))
        // 文字数は UTF-16 ではなくコードポイントで数える (Rust の chars と同じ)
        assertEquals("😀".repeat(64), normalizeDeviceName("😀".repeat(64)))
    }

    @Test
    fun theCodeIsNotPrinted() {
        val target = parseManualEntry("100.64.0.1", "", "ABCD2345").getOrThrow()
        assertFalse("ABCD2345" in target.toString())
    }
}
