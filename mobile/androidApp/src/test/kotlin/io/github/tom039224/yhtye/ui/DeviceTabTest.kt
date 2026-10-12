package io.github.tom039224.yhtye.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.lifecycle.Lifecycle
import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.github.tom039224.yhtye.shared.sync.BackgroundMode
import io.github.tom039224.yhtye.shared.update.AvailableUpdate
import io.github.tom039224.yhtye.shared.update.UpdateState
import io.github.tom039224.yhtye.shared.version.AppVersion
import io.github.tom039224.yhtye.ui.settings.DevicePane
import io.github.tom039224.yhtye.ui.settings.NotificationState
import io.github.tom039224.yhtye.ui.settings.SystemAccess
import io.github.tom039224.yhtye.ui.settings.UpdatePane
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** 設定の「この端末」: 接続先、常駐方式の切り替えと保存、通知の許可の案内、電池の最適化、ペアリング解除。 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w400dp-h800dp")
class DeviceTabTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val h = MainHarness(rule)

    @After
    fun tearDown() = h.close()

    private class FakeSystem : SystemAccess {
        var notifications = NotificationState.Blocked
        var needsPermission = true
        var ignoringBattery = false
        var requested = 0
        var openedSettings = 0
        var batteryRequests = 0

        override fun notificationState() = notifications

        override fun needsNotificationPermission() = needsPermission

        override fun requestNotifications(onResult: () -> Unit) {
            requested++
            // 許可の画面で許可された
            notifications = NotificationState.Allowed
            needsPermission = false
            onResult()
        }

        override fun openNotificationSettings() {
            openedSettings++
        }

        override fun isIgnoringBatteryOptimizations() = ignoringBattery

        override fun requestIgnoreBatteryOptimizations() {
            batteryRequests++
            ignoringBattery = true
        }

        val opened = mutableListOf<String>()

        override fun openUrl(url: String) {
            opened += url
        }
    }

    private val system = FakeSystem()
    private val mode = MutableStateFlow(BackgroundMode.Always)
    private val chosen = mutableListOf<BackgroundMode>()

    private val credentials = Credentials("sid", listOf("pc.tailnet.ts.net", "100.64.0.9"), 1423, "dev-1", "Pixel 8a", "token")

    private val updateState = MutableStateFlow(UpdateState())
    private var checkNowTaps = 0
    private val enabledChoices = mutableListOf<Boolean>()
    private val updates = UpdatePane(
        updateState,
        onCheckNow = { checkNowTaps++ },
        onEnabled = { enabledChoices += it; updateState.update { s -> s.copy(enabled = it) } },
    )

    private fun openDevice(
        hostVersion: String = "",
        appVersion: AppVersion = AppVersion(0, 1, 0),
        withUpdates: Boolean = false,
    ) {
        h.device = DevicePane(credentials, mode, { chosen += it; mode.value = it }, system, appVersion, if (withUpdates) updates else null)
        h.open { this.hostVersion = hostVersion }
        if (hostVersion.isNotEmpty()) {
            // 前面の間に ping が PC の版を持ってくる
            h.sync.setForeground(true)
            h.settle { h.sync.state.value.host.version != null }
        }
        h.openLeft()
        rule.onNodeWithTag("rail-settings").performClick()
        h.settle { h.exists("settings") }
        rule.onNodeWithTag("settings-tab-device").performClick()
        h.settle { h.exists("device-section") }
    }

    private fun click(tag: String) {
        rule.onNodeWithTag(tag).performScrollTo().performClick()
        rule.waitForIdle()
    }

    private fun selected(tag: String): Boolean =
        rule.onNodeWithTag(tag).fetchSemanticsNode().config.getOrNull(SemanticsProperties.Selected) == true

    @Test
    fun `shows the pc the address the name of this device and the state of the connection`() {
        openDevice()
        rule.onNodeWithTag("device-address").assertTextContains("pc.tailnet.ts.net:1423", substring = true)
        rule.onNodeWithTag("device-name").assertTextContains("Pixel 8a", substring = true)
        rule.onNodeWithTag("device-state").assertTextContains("接続中", substring = true)
    }

    // ---- 版 (T-84) ----

    @Test
    fun `shows the version of the pc and the version of this device`() {
        openDevice(hostVersion = "0.1.4", appVersion = AppVersion(0, 1, 0))
        rule.onNodeWithTag("device-pc-version").assertTextContains("0.1.4")
        rule.onNodeWithTag("device-app-version").assertTextContains("0.1.0")
        // 0.1 が同じなので互換: 警告は出さない
        assertFalse(h.exists("device-version-mismatch"))
    }

    @Test
    fun `an incompatible pc is said so with both versions`() {
        openDevice(hostVersion = "0.2.0", appVersion = AppVersion(0, 1, 0))
        rule.onNodeWithTag("device-version-mismatch").assertTextContains("PC 0.2.0 / この端末 0.1.0", substring = true)
    }

    @Test
    fun `an old pc that reports no version shows unknown and no warning`() {
        openDevice(hostVersion = "")
        rule.onNodeWithTag("device-pc-version").assertTextContains("不明")
        rule.onNodeWithTag("device-app-version").assertTextContains("0.1.0")
        assertFalse(h.exists("device-version-mismatch"))
    }

    // ---- アプリの更新 (T-84) ----

    @Test
    fun `without an update check there is no update section`() {
        openDevice()
        assertFalse(h.exists("device-update"))
    }

    @Test
    fun `a new version is announced and the download button opens its release page in the browser`() {
        updateState.value = UpdateState(checkedAt = 1_700_000_000_000L, available = AvailableUpdate(AppVersion(0, 1, 1), "android-v0.1.1"))
        openDevice(withUpdates = true)
        rule.onNodeWithTag("update-available").performScrollTo().assertTextContains("新しい版 0.1.1 があります")
        click("update-download")
        assertEquals(listOf("https://github.com/Tom039224/yhtye/releases/tag/android-v0.1.1"), system.opened)
    }

    @Test
    fun `when this app is the newest it says so and offers no download`() {
        updateState.value = UpdateState(checkedAt = 1_700_000_000_000L)
        openDevice(withUpdates = true)
        rule.onNodeWithTag("update-up-to-date").performScrollTo().assertTextContains("最新の版です", substring = true)
        rule.onNodeWithTag("update-checked").assertTextContains("最後に確認:", substring = true)
        assertFalse(h.exists("update-available"))
        assertFalse(h.exists("update-download"))
    }

    @Test
    fun `before the first check it says it has not checked yet`() {
        openDevice(withUpdates = true)
        rule.onNodeWithTag("update-checked").performScrollTo().assertTextContains("まだ確認していません", substring = true)
        assertFalse(h.exists("update-up-to-date"))
    }

    @Test
    fun `check now asks for a check and is off while one runs`() {
        openDevice(withUpdates = true)
        rule.onNodeWithTag("update-check-now").performScrollTo().assertIsEnabled()
        click("update-check-now")
        assertEquals(1, checkNowTaps)
        updateState.update { it.copy(checking = true) }
        rule.waitForIdle()
        rule.onNodeWithTag("update-check-now").assertIsNotEnabled()
        rule.onNodeWithTag("update-checked").assertTextContains("確認しています", substring = true)
    }

    @Test
    fun `a failed check shows its reason only here and keeps the last result`() {
        updateState.value = UpdateState(checkedAt = 1_700_000_000_000L, failure = "HTTP 403 (rate limited?)")
        openDevice(withUpdates = true)
        rule.onNodeWithTag("update-failure").performScrollTo().assertTextContains("確認できませんでした: HTTP 403 (rate limited?)")
        rule.onNodeWithTag("update-checked").assertTextContains("最後に確認:", substring = true)
        assertFalse(h.exists("update-up-to-date"))
    }

    @Test
    fun `the check can be turned off and then check now is off too`() {
        openDevice(withUpdates = true)
        click("update-enabled")
        assertEquals(listOf(false), enabledChoices)
        rule.waitForIdle()
        rule.onNodeWithTag("update-check-now").assertIsNotEnabled()
        click("update-enabled")
        assertEquals(listOf(false, true), enabledChoices)
        rule.onNodeWithTag("update-check-now").assertIsEnabled()
    }

    @Test
    fun `the background mode starts as always and a tap on the other one chooses and keeps it`() {
        openDevice()
        assertTrue(selected("mode-always"))
        assertFalse(selected("mode-work_only"))
        click("mode-work_only")
        assertEquals(listOf(BackgroundMode.WorkOnly), chosen)
        assertTrue(selected("mode-work_only"))
        assertFalse(selected("mode-always"))
        click("mode-always")
        assertEquals(listOf(BackgroundMode.WorkOnly, BackgroundMode.Always), chosen)
        assertTrue(selected("mode-always"))
    }

    @Test
    fun `blocked notifications are said so and the allow button asks and then shows allowed`() {
        openDevice()
        rule.onNodeWithTag("notifications-state").assertTextContains("許可されていません", substring = true)
        click("notifications-allow")
        assertEquals(1, system.requested)
        rule.onNodeWithTag("notifications-state").assertTextContains("許可されています", substring = true)
        assertFalse(h.exists("notifications-allow"))
    }

    @Test
    fun `the os notification settings can always be opened`() {
        openDevice()
        click("notifications-open-settings")
        assertEquals(1, system.openedSettings)
    }

    @Test
    fun `allowed notifications show no allow button`() {
        system.notifications = NotificationState.Allowed
        system.needsPermission = false
        openDevice()
        rule.onNodeWithTag("notifications-state").assertTextContains("許可されています", substring = true)
        assertFalse(h.exists("notifications-allow"))
        assertTrue(h.exists("notifications-open-settings"))
    }

    @Test
    fun `battery optimization is explained with a button until the app is exempt`() {
        openDevice()
        rule.onNodeWithTag("battery-state").assertTextContains("最適化の対象です", substring = true)
        click("battery-request")
        assertEquals(1, system.batteryRequests)
        // OS の確認の画面から戻ったら (アクティビティが再開したら) 読み直す
        rule.activityRule.scenario.moveToState(Lifecycle.State.STARTED)
        rule.activityRule.scenario.moveToState(Lifecycle.State.RESUMED)
        rule.waitForIdle()
        rule.onNodeWithTag("battery-state").assertTextContains("対象外です", substring = true)
        assertFalse(h.exists("battery-request"))
    }

    @Test
    fun `unpairing asks first and then unpairs`() {
        openDevice()
        click("unpair")
        // 確認だけでは解除しない
        assertEquals(0, h.unpaired)
        rule.onNodeWithTag("unpairConfirm").performClick()
        rule.waitForIdle()
        assertEquals(1, h.unpaired)
    }

    @Test
    fun `cancelling the unpair dialog keeps the pairing`() {
        openDevice()
        click("unpair")
        rule.onNodeWithTag("unpairConfirm").assertExists()
        rule.onNodeWithText("キャンセル").performClick()
        rule.waitForIdle()
        assertFalse(h.exists("unpairConfirm"))
        assertEquals(0, h.unpaired)
    }
}
