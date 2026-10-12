package io.github.tom039224.yhtye.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.lifecycle.Lifecycle
import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.github.tom039224.yhtye.shared.sync.BackgroundMode
import io.github.tom039224.yhtye.ui.settings.DevicePane
import io.github.tom039224.yhtye.ui.settings.NotificationState
import io.github.tom039224.yhtye.ui.settings.SystemAccess
import kotlinx.coroutines.flow.MutableStateFlow
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
    }

    private val system = FakeSystem()
    private val mode = MutableStateFlow(BackgroundMode.Always)
    private val chosen = mutableListOf<BackgroundMode>()

    private val credentials = Credentials("sid", listOf("pc.tailnet.ts.net", "100.64.0.9"), 1423, "dev-1", "Pixel 8a", "token")

    private fun openDevice() {
        h.device = DevicePane(credentials, mode, { chosen += it; mode.value = it }, system)
        h.open()
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
