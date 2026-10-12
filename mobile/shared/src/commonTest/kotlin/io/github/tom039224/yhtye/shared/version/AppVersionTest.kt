package io.github.tom039224.yhtye.shared.version

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class AppVersionTest {
    private fun v(text: String) = AppVersion.parse(text) ?: error("not a version: $text")

    @Test
    fun readsMajorMinorPatch() {
        assertEquals(AppVersion(0, 1, 0), AppVersion.parse("0.1.0"))
        assertEquals(AppVersion(12, 34, 56), AppVersion.parse(" 12.34.56 "))
    }

    @Test
    fun readsAPreReleaseAndIgnoresBuildMetadata() {
        assertEquals(AppVersion(1, 2, 3, "beta.1"), AppVersion.parse("1.2.3-beta.1"))
        assertEquals(AppVersion(1, 2, 3, "rc-1"), AppVersion.parse("1.2.3-rc-1+build.5"))
        assertEquals(AppVersion(1, 2, 3), AppVersion.parse("1.2.3+build.5"))
    }

    @Test
    fun refusesWhatIsNotAVersion() {
        listOf(
            "", "1", "1.2", "1.2.3.4", "v1.2.3", "01.2.3", "1.02.3", "1.2.03", "-1.2.3", "1.2.x", "1.2.3-", "1.2.3-01",
            "1.2.3-a..b", "1.2.3 beta", "android-v1.2.3", "99999999999.0.0",
        ).forEach { assertNull(AppVersion.parse(it), it) }
    }

    @Test
    fun printsTheWayItIsRead() {
        listOf("0.1.0", "10.20.30", "1.0.0-alpha.1").forEach { assertEquals(it, v(it).toString()) }
    }

    @Test
    fun comparesTheNumbersNotTheText() {
        assertTrue(v("0.10.0") > v("0.9.9"))
        assertTrue(v("1.0.0") > v("0.99.99"))
        assertTrue(v("0.1.10") > v("0.1.9"))
        assertEquals(0, v("1.2.3").compareTo(v("1.2.3")))
    }

    @Test
    fun aPreReleaseIsOlderThanTheReleaseOfTheSameVersion() {
        assertTrue(v("1.0.0-rc.1") < v("1.0.0"))
        assertTrue(v("0.1.1") > v("0.1.0"))
        // 正式版 0.1.0 は 0.1.1 のプレリリースより古い
        assertTrue(v("0.1.0") < v("0.1.1-beta"))
    }

    @Test
    fun ordersPreReleasesAsSemanticVersioningDoes() {
        // semver.org の例の並び
        val ordered = listOf(
            "1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta", "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0",
        ).map(::v)
        for (i in ordered.indices) {
            for (j in ordered.indices) {
                assertEquals(i.compareTo(j), ordered[i].compareTo(ordered[j]).coerceIn(-1, 1), "${ordered[i]} vs ${ordered[j]}")
            }
        }
    }

    @Test
    fun buildMetadataDoesNotChangeTheOrder() {
        assertEquals(0, v("1.2.3+a").compareTo(v("1.2.3+b")))
    }

    @Test
    fun comparesAVeryLongNumericIdentifierWithoutOverflowing() {
        assertTrue(v("1.0.0-123456789012345678901234567890") > v("1.0.0-99999999999999999999"))
    }
}
