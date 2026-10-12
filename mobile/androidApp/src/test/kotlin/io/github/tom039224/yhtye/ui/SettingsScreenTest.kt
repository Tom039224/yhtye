package io.github.tom039224.yhtye.ui

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.hasAnyDescendant
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import io.github.tom039224.yhtye.shared.api.generated.AgentChoice
import io.github.tom039224.yhtye.shared.api.generated.AgentRole
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.Candidate
import io.github.tom039224.yhtye.shared.api.generated.EffortOption
import io.github.tom039224.yhtye.shared.api.generated.HarnessDetection
import io.github.tom039224.yhtye.shared.api.generated.HarnessModels
import io.github.tom039224.yhtye.shared.api.generated.HarnessPathSource
import io.github.tom039224.yhtye.shared.api.generated.HarnessRequirement
import io.github.tom039224.yhtye.shared.api.generated.ModelOption
import io.github.tom039224.yhtye.shared.api.generated.SettingsKind
import io.github.tom039224.yhtye.shared.sync.CONFLICT_MESSAGE
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * 設定画面 (PC の設定と同じ 3 つの節): 読み込み・保存・`conflict`・`settings_changed` での読み直しと「再読込」のバナー。
 * 本物の同期層を偽のコア ([TestCore]) の上で動かし、操作がコアへの要求になることを確かめる。
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "w400dp-h800dp")
class SettingsScreenTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val h = MainHarness(rule)

    @After
    fun tearDown() = h.close()

    private fun TestCore.withModels() {
        harnessModels["claude-code"] = HarnessModels(
            "claude-code",
            listOf(
                ModelOption("haiku", "Haiku"),
                ModelOption("sonnet", "Sonnet", efforts = listOf(EffortOption("medium", "Medium"), EffortOption("high", "High"))),
                ModelOption("opus", "Opus", efforts = listOf(EffortOption("high", "High"))),
            ),
            null,
            0,
        )
        harnessModels["codex"] = HarnessModels("codex", listOf(ModelOption("gpt-5", "GPT-5")), null, 0)
        detections = listOf(
            HarnessDetection(
                "claude-code", "Claude Code", true, "/usr/bin/npx", HarnessPathSource.Path, null, null,
                listOf(HarnessRequirement("npx", "/usr/bin/npx")),
            ),
            HarnessDetection(
                "devin", "Devin", false, null, HarnessPathSource.None, null, null,
                listOf(HarnessRequirement("devin", null)),
            ),
        )
        secrets += "STRIPE_TEST_KEY"
    }

    private fun openSettings(tab: String? = null, configure: TestCore.() -> Unit = {}) {
        h.open {
            withModels()
            configure()
        }
        h.openLeft()
        rule.onNodeWithTag("rail-settings").performClick()
        h.settle { h.exists("settings") }
        if (tab != null) {
            rule.onNodeWithTag("settings-tab-$tab").performClick()
            rule.waitForIdle()
        }
    }

    private fun click(tag: String) {
        rule.onNodeWithTag(tag).performScrollTo().performClick()
        rule.waitForIdle()
    }

    private fun settled(done: () -> Boolean) = h.settle(done)

    /** 「再読込」(スクロールの外に固定したバナーのボタン)。 */
    private fun tapBanner() {
        rule.onNodeWithTag("settings-reload").performClick()
        rule.waitForIdle()
    }

    /** [tag] の下のどこかに [text] を含む文字がある (カードのように中の文字を一つにまとめない部品の確認)。 */
    private fun assertShows(tag: String, text: String) {
        rule.onNode(hasTestTag(tag) and hasAnyDescendant(hasText(text, substring = true))).assertExists()
    }

    private fun saves() = h.core.callsOf<ApiCommand.SetAgentSettings>()

    private fun openAgents(configure: TestCore.() -> Unit = {}) {
        openSettings("agents", configure)
        settled { h.exists("role-implementer") && h.core.callsOf<ApiCommand.ListHarnessModels>().size >= 2 }
        // 全体の層で見る (プロジェクトの層は別のテスト)
        click("scope-global")
        settled { h.core.callsOf<ApiCommand.GetAgentSettings>().any { it.project == null } }
    }

    private fun implementerRowsElsewhere(vararg rows: Candidate) = h.core.changeAgentSettingsElsewhere {
        it.copy(effective = it.effective.copy(implementer = it.effective.implementer.copy(candidates = rows.toList())))
    }

    // ---- エージェント ----

    @Test
    fun `shows the roles with their rows the default star and the models read from the harnesses`() {
        openAgents()
        for (role in listOf("orchestrator", "implementer", "investigator", "reviewer")) rule.onNodeWithTag("role-$role").assertExists()
        rule.onNodeWithTag("summary-implementer-0").assertTextContains("Claude Code · Sonnet (sonnet) · effort medium", substring = true)
        rule.onNodeWithTag("summary-implementer-0").assertTextContains("ふつうの変更", substring = true)
        rule.onNodeWithTag("summary-implementer-1").assertTextContains("Opus (opus) · effort high", substring = true)
        // 既定の行の★は押せない。もう一方は押せる
        rule.onNodeWithTag("default-implementer-0").assertIsNotEnabled()
        rule.onNodeWithTag("default-implementer-1").assertIsEnabled()
        rule.onNodeWithTag("inherit-orchestrator").assertExists()
    }

    @Test
    fun `the star makes a row the default and saves with the revision that is shown`() {
        openAgents()
        click("default-implementer-1")
        settled { saves().isNotEmpty() }
        val sent = saves().single()
        assertEquals(AgentRole.Implementer, sent.role)
        assertEquals(1_000L, sent.expectedRevision)
        assertNull(sent.project)
        assertEquals(AgentChoice("claude-code", "opus", "high"), sent.settings?.default)
        settled { h.core.agentRevision == 1_001L && !rule.isEnabled("default-implementer-1") }
        rule.onNodeWithTag("default-implementer-0").assertIsEnabled()
    }

    private fun androidx.compose.ui.test.junit4.ComposeTestRule.isEnabled(tag: String): Boolean =
        runCatching { onNodeWithTag(tag).assertIsEnabled() }.isSuccess

    @Test
    fun `an open row changes the model and drops an effort the model does not have`() {
        openAgents()
        click("summary-implementer-0")
        click("model-implementer-0")
        click("model-implementer-0-opus")
        settled { saves().isNotEmpty() }
        val row = saves().single().settings!!.candidates[0]
        // opus には medium が無い
        assertEquals(Candidate("claude-code", "opus", null, "ふつうの変更"), row)
        // 既定の行だったので既定も付いていく
        assertEquals(AgentChoice("claude-code", "opus", null), saves().single().settings?.default)
    }

    @Test
    fun `a duplicate combination is refused with the reason and nothing is sent`() {
        openAgents()
        click("summary-implementer-1")
        click("model-implementer-1")
        click("model-implementer-1-sonnet")
        // 2 行目が sonnet になる。effort は opus の high に合わせて消えるので同じ組み合わせにはならず保存される
        settled { saves().isNotEmpty() }
        click("effort-implementer-1")
        click("effort-implementer-1-medium")
        settled { h.exists("agent-error") }
        rule.onNodeWithTag("agent-error").assertTextContains("同じハーネス・モデル・effort の行が既にあります", substring = true)
        assertEquals(1, saves().size)
    }

    @Test
    fun `changing the harness takes the first model of that harness and clears the effort`() {
        openAgents()
        click("summary-implementer-0")
        click("harness-implementer-0")
        click("harness-implementer-0-codex")
        settled { saves().isNotEmpty() }
        assertEquals(Candidate("codex", "gpt-5", null, "ふつうの変更"), saves().single().settings!!.candidates[0])
    }

    @Test
    fun `the effort list comes from the model and can be cleared`() {
        openAgents()
        click("summary-implementer-0")
        click("effort-implementer-0")
        click("effort-implementer-0-high")
        settled { saves().isNotEmpty() }
        assertEquals("high", saves().single().settings!!.candidates[0].effort)
        settled { h.core.agentRevision == 1_001L }
        click("effort-implementer-0")
        click("effort-implementer-0-")
        settled { saves().size == 2 }
        assertNull(saves().last().settings!!.candidates[0].effort)
    }

    @Test
    fun `a model without efforts cannot be given one`() {
        openAgents()
        implementerRowsElsewhere(Candidate("claude-code", "haiku", null, ""))
        h.core.emit(appWide(ApiEventBody.SettingsChanged(SettingsKind.AgentSettings, null, h.core.agentRevision)))
        settled { runCatching { rule.onNodeWithTag("summary-implementer-0").assertTextContains("Haiku (haiku)", substring = true) }.isSuccess }
        click("summary-implementer-0")
        settled { h.exists("effort-implementer-0") }
        rule.onNodeWithTag("effort-implementer-0").assertIsNotEnabled()
    }

    @Test
    fun `adding a row takes the first unused combination and opens it`() {
        openAgents()
        click("add-row-implementer")
        settled { saves().isNotEmpty() }
        val rows = saves().single().settings!!.candidates
        assertEquals(3, rows.size)
        // sonnet/medium と opus/high がある: 最後の行 (claude-code) の最初の未使用は haiku (effort なし)
        assertEquals(Candidate("claude-code", "haiku", null, ""), rows[2])
    }

    @Test
    fun `removing a row keeps the default and the last row cannot be removed`() {
        openAgents()
        click("remove-implementer-0")
        settled { saves().isNotEmpty() }
        val left = saves().single().settings!!
        assertEquals(listOf("opus"), left.candidates.map { it.model })
        // 既定を消したので残りの先頭が既定
        assertEquals(AgentChoice("claude-code", "opus", "high"), left.default)
        settled { h.core.agentRevision == 1_001L && !h.exists("summary-implementer-1") }
        rule.onNodeWithTag("remove-implementer-0").assertIsNotEnabled()
    }

    @Test
    fun `the note is saved when the keyboard is done`() {
        openAgents()
        click("summary-implementer-1")
        rule.onNodeWithTag("note-implementer-1").performScrollTo().performTextInput("設計が絡む難しい変更に")
        rule.onNodeWithTag("note-implementer-1").performImeAction()
        settled { saves().isNotEmpty() }
        assertEquals("設計が絡む難しい変更に", saves().single().settings!!.candidates[1].note)
    }

    @Test
    fun `a role can go back to the built-in default and take its own settings again`() {
        openAgents()
        click("inherit-implementer")
        settled { saves().isNotEmpty() }
        assertNull(saves().single().settings)
        assertEquals(AgentRole.Implementer, saves().single().role)
        settled { h.core.agentRevision == 1_001L }
    }

    @Test
    fun `the project layer inherits the global settings until it is given its own`() {
        openSettings("agents")
        settled { h.exists("role-implementer") }
        click("scope-project")
        settled { h.core.callsOf<ApiCommand.GetAgentSettings>().any { it.project == "repo" } }
        settled { h.exists("summary-implementer-0") }
        // 継承の間は行を編集できない
        rule.onNodeWithTag("default-implementer-1").assertIsNotEnabled()
        click("inherit-implementer")
        settled { saves().isNotEmpty() }
        val sent = saves().single()
        assertEquals("repo", sent.project)
        assertEquals(listOf("sonnet", "opus"), sent.settings!!.candidates.map { it.model })
        settled { h.core.agentRevision == 1_001L }
        // 全体の層はそのまま
        assertEquals(listOf("sonnet", "opus"), h.core.agentView.global.implementer!!.candidates.map { it.model })
    }

    @Test
    fun `a save after another device changed the settings says so and shows the latest`() {
        openAgents()
        // 他の端末が先に保存した (版が進む)。知らせはまだ届いていない
        implementerRowsElsewhere(Candidate("claude-code", "haiku", null, "他の端末で"))
        click("default-implementer-1")
        settled { h.exists("agent-error") }
        rule.onNodeWithTag("agent-error").assertTextEquals(CONFLICT_MESSAGE)
        // 保存していない (版が違った) し、最新の設定を表示している
        assertEquals(0, h.core.agentSaves)
        rule.onNodeWithTag("summary-implementer-0").assertTextContains("他の端末で", substring = true)
    }

    @Test
    fun `settings changed on another device are read again`() {
        openAgents()
        val reads = h.core.callsOf<ApiCommand.GetAgentSettings>().size
        implementerRowsElsewhere(Candidate("claude-code", "haiku", null, "PC で変えた"))
        h.core.emit(appWide(ApiEventBody.SettingsChanged(SettingsKind.AgentSettings, null, h.core.agentRevision)))
        settled { h.core.callsOf<ApiCommand.GetAgentSettings>().size > reads }
        settled { runCatching { rule.onNodeWithTag("summary-implementer-0").assertTextContains("PC で変えた", substring = true) }.isSuccess }
        assertFalse(h.exists("settings-stale"))
    }

    @Test
    fun `while a note is being typed a change elsewhere shows the banner and the reload button reads it`() {
        openAgents()
        click("summary-implementer-0")
        rule.onNodeWithTag("note-implementer-0").performScrollTo().performTextInput("書きかけ")
        val reads = h.core.callsOf<ApiCommand.GetAgentSettings>().size
        h.core.changeAgentSettingsElsewhere()
        h.core.emit(appWide(ApiEventBody.SettingsChanged(SettingsKind.AgentSettings, null, h.core.agentRevision)))
        settled { h.exists("settings-stale") }
        // 読み直していない (書きかけの文字を守る)
        assertEquals(reads, h.core.callsOf<ApiCommand.GetAgentSettings>().size)
        tapBanner()
        settled { !h.exists("settings-stale") && h.core.callsOf<ApiCommand.GetAgentSettings>().size > reads }
    }

    @Test
    fun `a failed save shows the reason the core gave`() {
        openAgents()
        h.core.fail(ApiCommand.SetAgentSettings::class, "unknown harness: codex")
        click("default-implementer-1")
        settled { h.exists("agent-error") }
        rule.onNodeWithTag("agent-error").assertTextEquals("unknown harness: codex")
    }

    // ---- ハーネス ----

    @Test
    fun `lists the harnesses with where they were found and a manual path form`() {
        openSettings("harnesses")
        settled { h.exists("harness-claude-code") }
        assertShows("harness-claude-code", "インストール済み")
        assertShows("harness-claude-code", "/usr/bin/npx")
        assertShows("harness-claude-code", "PATH")
        assertShows("harness-devin", "未インストール")
        assertShows("harness-devin", "見つかりません")
    }

    @Test
    fun `a manual path is saved and can be removed and an empty one is refused`() {
        openSettings("harnesses")
        settled { h.exists("harness-devin") }
        rule.onNodeWithTag("path-devin").performScrollTo().performTextInput("/opt/bin/devin")
        click("path-save-devin")
        settled { h.core.callsOf<ApiCommand.SetHarnessPath>().isNotEmpty() }
        assertEquals(ApiCommand.SetHarnessPath("devin", "/opt/bin/devin"), h.core.callsOf<ApiCommand.SetHarnessPath>().single())
        settled { rule.isEnabled("path-reset-devin") }
        click("path-reset-devin")
        settled { h.core.callsOf<ApiCommand.SetHarnessPath>().size == 2 }
        assertNull(h.core.callsOf<ApiCommand.SetHarnessPath>().last().path)
        // 空のまま保存は送らず、理由を出す
        click("path-save-devin")
        settled { h.exists("path-error-devin") }
        rule.onNodeWithTag("path-error-devin").assertTextContains("パスを入力してください", substring = true)
        assertEquals(2, h.core.callsOf<ApiCommand.SetHarnessPath>().size)
    }

    @Test
    fun `a refused path shows the cores reason on its row`() {
        openSettings("harnesses")
        settled { h.exists("harness-devin") }
        h.core.fail(ApiCommand.SetHarnessPath::class, "not an executable file")
        rule.onNodeWithTag("path-devin").performScrollTo().performTextInput("/etc/passwd")
        click("path-save-devin")
        settled { h.exists("path-error-devin") }
        rule.onNodeWithTag("path-error-devin").assertTextContains("not an executable file", substring = true)
    }

    @Test
    fun `detecting again asks the core and a change elsewhere reads the list again unless a path is typed`() {
        openSettings("harnesses")
        settled { h.exists("harness-devin") }
        click("detect-harnesses")
        settled { h.core.callsOf<ApiCommand.DetectHarnesses>().isNotEmpty() }
        val reads = h.core.callsOf<ApiCommand.GetHarnesses>().size
        h.core.emit(appWide(ApiEventBody.SettingsChanged(SettingsKind.Harnesses)))
        settled { h.core.callsOf<ApiCommand.GetHarnesses>().size > reads }
        // パスを入力中は読み直さずバナー
        rule.onNodeWithTag("path-devin").performScrollTo().performTextInput("/opt/bin")
        val now = h.core.callsOf<ApiCommand.GetHarnesses>().size
        h.core.emit(appWide(ApiEventBody.SettingsChanged(SettingsKind.Harnesses)))
        settled { h.exists("settings-stale") }
        assertEquals(now, h.core.callsOf<ApiCommand.GetHarnesses>().size)
        tapBanner()
        settled { !h.exists("settings-stale") && h.core.callsOf<ApiCommand.GetHarnesses>().size > now }
    }

    // ---- 秘密の環境変数 ----

    @Test
    fun `lists the names and never shows a value`() {
        openSettings("secrets")
        settled { h.exists("secret-row-STRIPE_TEST_KEY") }
        assertShows("secret-row-STRIPE_TEST_KEY", "STRIPE_TEST_KEY")
        assertShows("secret-row-STRIPE_TEST_KEY", "登録済み")
    }

    @Test
    fun `registers a variable sends the value once and clears the form`() {
        openSettings("secrets")
        settled { h.exists("secret-row-STRIPE_TEST_KEY") }
        rule.onNodeWithTag("secret-name").performTextInput("OPENROUTER_API_KEY")
        rule.onNodeWithTag("secret-value").performTextInput("sk-secret")
        click("secret-register")
        settled { h.core.callsOf<ApiCommand.SetSecretEnv>().isNotEmpty() }
        assertEquals(ApiCommand.SetSecretEnv("OPENROUTER_API_KEY", "sk-secret"), h.core.callsOf<ApiCommand.SetSecretEnv>().single())
        settled { h.exists("secret-row-OPENROUTER_API_KEY") }
        // フォームは空に戻る (入力欄の文字は無い)
        rule.onNodeWithTag("secret-name").assertTextEquals("NAME (例 OPENROUTER_API_KEY_CODEX)", includeEditableText = false)
    }

    @Test
    fun `a bad name or an empty value is refused before anything is sent`() {
        openSettings("secrets")
        settled { h.exists("secret-row-STRIPE_TEST_KEY") }
        rule.onNodeWithTag("secret-name").performTextInput("1BAD")
        rule.onNodeWithTag("secret-value").performTextInput("v")
        click("secret-register")
        rule.onNodeWithTag("secret-error").assertTextContains("数字から始められません", substring = true)
        assertEquals(emptyList(), h.core.callsOf<ApiCommand.SetSecretEnv>())
    }

    @Test
    fun `a failed registration shows the reason and drops the value`() {
        openSettings("secrets")
        settled { h.exists("secret-row-STRIPE_TEST_KEY") }
        h.core.failWith(ApiCommand.SetSecretEnv::class, ApiErrorCode.Unavailable, "no Secret Service")
        rule.onNodeWithTag("secret-name").performTextInput("GOOD_NAME")
        rule.onNodeWithTag("secret-value").performTextInput("secret")
        click("secret-register")
        settled { h.exists("secret-error") }
        rule.onNodeWithTag("secret-error").assertTextContains("no Secret Service", substring = true)
        // 値の欄は空 (入れた文字は残らない)
        rule.onNodeWithTag("secret-value").assertTextEquals("値", includeEditableText = false)
    }

    @Test
    fun `deleting asks first and overwriting fills the name`() {
        openSettings("secrets")
        settled { h.exists("secret-row-STRIPE_TEST_KEY") }
        click("secret-overwrite-STRIPE_TEST_KEY")
        rule.onNodeWithTag("secret-name").assertTextContains("STRIPE_TEST_KEY", substring = true)
        click("secret-delete-STRIPE_TEST_KEY")
        assertShows("secret-row-STRIPE_TEST_KEY", "を削除しますか?")
        assertEquals(emptyList(), h.core.callsOf<ApiCommand.DeleteSecretEnv>())
        click("secret-delete-cancel")
        click("secret-delete-STRIPE_TEST_KEY")
        click("secret-delete-confirm")
        settled { h.core.callsOf<ApiCommand.DeleteSecretEnv>().isNotEmpty() }
        assertEquals("STRIPE_TEST_KEY", h.core.callsOf<ApiCommand.DeleteSecretEnv>().single().name)
        settled { h.exists("secret-empty") }
    }

    @Test
    fun `names added on another device appear and a banner shows while the form is being filled`() {
        openSettings("secrets")
        settled { h.exists("secret-row-STRIPE_TEST_KEY") }
        h.core.secrets += "FROM_THE_PC"
        h.core.emit(appWide(ApiEventBody.SettingsChanged(SettingsKind.Secrets)))
        settled { h.exists("secret-row-FROM_THE_PC") }
        rule.onNodeWithTag("secret-name").performTextInput("TYPING")
        rule.onNodeWithTag("secret-name").assertTextContains("TYPING")
        h.core.secrets += "ANOTHER"
        h.core.emit(appWide(ApiEventBody.SettingsChanged(SettingsKind.Secrets)))
        settled { h.exists("settings-stale") }
        assertFalse(h.exists("secret-row-ANOTHER"))
        tapBanner()
        settled { h.exists("secret-row-ANOTHER") }
        assertTrue(h.exists("secret-row-ANOTHER"))
    }
}
