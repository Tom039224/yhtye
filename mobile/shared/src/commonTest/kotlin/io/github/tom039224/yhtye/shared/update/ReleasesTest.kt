package io.github.tom039224.yhtye.shared.update

import io.github.tom039224.yhtye.shared.version.AppVersion
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class ReleasesTest {
    private fun v(text: String) = AppVersion.parse(text) ?: error(text)

    private fun release(tag: String, draft: Boolean = false, prerelease: Boolean = true) = GithubRelease(tag, draft, prerelease)

    @Test
    fun picksTheNewestAndroidReleaseThatIsNewerThanThisApp() {
        val releases = listOf(
            release("android-v0.1.1"),
            release("android-v0.3.0"),
            release("android-v0.2.0"),
        )
        assertEquals(AvailableUpdate(v("0.3.0"), "android-v0.3.0"), newestAndroidUpdate(releases, v("0.1.0")))
    }

    @Test
    fun comparesVersionsAsNumbersSoTenIsNewerThanNine() {
        val releases = listOf(release("android-v0.1.9"), release("android-v0.1.10"))
        assertEquals(v("0.1.10"), newestAndroidUpdate(releases, v("0.1.0"))?.version)
    }

    @Test
    fun saysNothingWhenThisAppIsAlreadyTheNewestOrNewer() {
        val releases = listOf(release("android-v0.1.0"), release("android-v0.1.1"))
        assertNull(newestAndroidUpdate(releases, v("0.1.1")))
        assertNull(newestAndroidUpdate(releases, v("0.2.0")))
        assertNull(newestAndroidUpdate(emptyList(), v("0.1.0")))
    }

    @Test
    fun includesReleasesMarkedAsPreReleaseOnGitHub() {
        // 0.x の間のリリースは全部プレリリースとして出す。それでも知らせる
        assertEquals(v("0.1.1"), newestAndroidUpdate(listOf(release("android-v0.1.1", prerelease = true)), v("0.1.0"))?.version)
        assertEquals(v("0.1.1"), newestAndroidUpdate(listOf(release("android-v0.1.1", prerelease = false)), v("0.1.0"))?.version)
    }

    @Test
    fun skipsDraftsAndTagsThatAreNotAndroidReleases() {
        val releases = listOf(
            release("android-v9.0.0", draft = true),
            release("v9.0.0"),
            release("desktop-v9.0.0"),
            release("android-9.0.0"),
            release("android-vnext"),
            release("android-v9.0"),
            release("android-v 9.0.0"),
            release("android-v0.1.1"),
        )
        assertEquals(v("0.1.1"), newestAndroidUpdate(releases, v("0.1.0"))?.version)
    }

    @Test
    fun aPreReleaseVersionIsNewerThanTheOlderReleaseButOlderThanItsOwnRelease() {
        val releases = listOf(release("android-v0.2.0-beta.1"))
        assertEquals(v("0.2.0-beta.1"), newestAndroidUpdate(releases, v("0.1.0"))?.version)
        assertNull(newestAndroidUpdate(releases, v("0.2.0")))
    }

    @Test
    fun theReleasePageIsBuiltFromTheTagNotFromTheResponse() {
        assertEquals("https://github.com/Tom039224/yhtye/releases/tag/android-v0.1.1", androidUpdateOfTag("android-v0.1.1")?.pageUrl)
    }

    @Test
    fun theUpdateEndpointsAreHttps() {
        assertEquals(true, RELEASES_URL.startsWith("https://api.github.com/repos/Tom039224/yhtye/releases"))
        assertEquals(true, RELEASE_PAGE_URL.startsWith("https://github.com/Tom039224/yhtye/releases/"))
    }
}
