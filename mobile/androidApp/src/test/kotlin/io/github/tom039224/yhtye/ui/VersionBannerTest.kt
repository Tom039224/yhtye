package io.github.tom039224.yhtye.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import io.github.tom039224.yhtye.shared.sync.BackgroundMode
import io.github.tom039224.yhtye.shared.update.AvailableUpdate
import io.github.tom039224.yhtye.shared.update.UpdateState
import io.github.tom039224.yhtye.shared.version.AppVersion
import io.github.tom039224.yhtye.ui.settings.DevicePane
import io.github.tom039224.yhtye.ui.settings.SystemAccess
import io.github.tom039224.yhtye.ui.settings.UpdatePane
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertFalse

/**
 * PC の版とこの端末の版が合わないときの会話画面のバナー (接続のバナーと同じ場所) と、新しい版があるときの
 * 左パネルの設定のボタンの印 (T-84)。
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w400dp-h800dp")
class VersionBannerTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val h = MainHarness(rule)

    @After
    fun tearDown() = h.close()

    private val updateState = MutableStateFlow(UpdateState())

    /** [hostVersion] を返す PC に、版 [appVersion] のアプリでつなぐ。 */
    private fun open(hostVersion: String, appVersion: AppVersion = AppVersion(0, 1, 0)) {
        h.device = DevicePane(null, MutableStateFlow(BackgroundMode.DEFAULT), {}, SystemAccess.None, appVersion, UpdatePane(updateState, {}, {}))
        h.open { this.hostVersion = hostVersion }
        // ping は前面にある間だけ
        h.sync.setForeground(true)
        h.settle { h.sync.state.value.host.name != null }
    }

    private fun bannerHasText(text: String) =
        rule.onNode(hasText(text, substring = true) and hasAnyAncestor(hasTestTag("version-mismatch"))).assertExists()

    /** 印は押せるボタンの中なので、結合した木には現れない。 */
    private fun marked(): Boolean = rule.onAllNodes(hasTestTag("rail-settings-mark"), useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()

    // ---- 版が合わないバナー ----

    @Test
    fun `a pc whose minor version differs before 1_0 gets a warning on top of the conversation`() {
        open(hostVersion = "0.2.0")
        bannerHasText("PC の Yhtye と版が合いません (PC 0.2.0 / この端末 0.1.0)。どちらかを更新してください")
    }

    @Test
    fun `the warning also shows when the pc is older than this device`() {
        open(hostVersion = "0.1.0", appVersion = AppVersion(0, 2, 0))
        bannerHasText("PC 0.1.0 / この端末 0.2.0")
    }

    @Test
    fun `a pc on the same minor version gets no warning whatever its patch`() {
        open(hostVersion = "0.1.9")
        assertFalse(h.exists("version-mismatch"))
    }

    @Test
    fun `an old pc that does not report its version gets no warning`() {
        open(hostVersion = "")
        assertFalse(h.exists("version-mismatch"))
    }

    // ---- 設定のボタンの印 ----

    private fun settingsDescription(): String? =
        rule.onNodeWithTag("rail-settings").fetchSemanticsNode().config.getOrNull(SemanticsProperties.ContentDescription)?.single()

    @Test
    fun `a new version puts a small mark on the settings button`() {
        open(hostVersion = "0.1.0")
        h.openLeft()
        assertFalse(marked())
        assertEquals("設定", settingsDescription())
        updateState.value = UpdateState(available = AvailableUpdate(AppVersion(0, 1, 1), "android-v0.1.1"))
        h.settle { marked() }
        assertEquals("設定 (新しい版があります)", settingsDescription())
        updateState.value = UpdateState()
        h.settle { !marked() }
        assertEquals("設定", settingsDescription())
    }
}
