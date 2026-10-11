package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentChoice
import io.github.tom039224.yhtye.shared.api.generated.AgentRole
import io.github.tom039224.yhtye.shared.api.generated.AgentSettings
import io.github.tom039224.yhtye.shared.api.generated.AgentSettingsLayer
import io.github.tom039224.yhtye.shared.api.generated.AgentSettingsView
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.Candidate
import io.github.tom039224.yhtye.shared.api.generated.EffortOption
import io.github.tom039224.yhtye.shared.api.generated.HarnessDetection
import io.github.tom039224.yhtye.shared.api.generated.HarnessInfo
import io.github.tom039224.yhtye.shared.api.generated.HarnessModels
import io.github.tom039224.yhtye.shared.api.generated.HarnessPathSource
import io.github.tom039224.yhtye.shared.api.generated.HarnessRequirement
import io.github.tom039224.yhtye.shared.api.generated.ModelOption
import io.github.tom039224.yhtye.shared.api.generated.RoleSettings
import io.github.tom039224.yhtye.shared.api.generated.SettingsKind
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * 設定画面の 3 つの節 (PC の `AgentSettings.test.tsx` / `HarnessSettings.test.tsx` / `SecretEnv.test.tsx` と `SettingsSync.test.tsx`):
 * 読み込み・保存・`conflict`・`settings_changed` での読み直しと「再読込」のバナー (編集中・保存中)。
 */
@OptIn(ExperimentalCoroutinesApi::class)
class SettingsSectionsTest {
    private val core = FakeCore(PROJECT, chatSnapshot())

    private val sonnet = Candidate("claude-code", "sonnet", "medium", "ふつう")
    private val opus = Candidate("claude-code", "opus", "high", "")
    private fun role(vararg rows: Candidate) = RoleSettings(rows.toList(), rowChoice(rows[0]))

    private val global = role(sonnet, opus)

    init {
        core.agentView = AgentSettingsView(
            harnesses = listOf(HarnessInfo("claude-code", "Claude Code", false)),
            builtin = AgentChoice("claude-code", "haiku"),
            global = AgentSettingsLayer(implementer = global),
            effective = AgentSettings(global, global, global, global),
            revision = core.agentRevision,
        )
        core.harnessModels["claude-code"] = HarnessModels(
            "claude-code",
            listOf(ModelOption("sonnet", "Sonnet", efforts = listOf(EffortOption("medium", "Medium"))), ModelOption("opus", "Opus")),
            null,
            0,
        )
        core.modelEfforts[modelKey("claude-code", "opus")] = listOf(EffortOption("high", "High"))
        core.detections = listOf(
            HarnessDetection(
                "claude-code", "Claude Code", true, "/usr/bin/npx", HarnessPathSource.Path, null, null,
                listOf(HarnessRequirement("npx", "/usr/bin/npx")),
            ),
            HarnessDetection(
                "devin", "Devin", false, null, HarnessPathSource.None, null, null,
                listOf(HarnessRequirement("devin", null)),
            ),
        )
    }

    private fun TestScope.agent(): AgentSettingsController {
        val store = newStore(core)
        return AgentSettingsController(store.settings, backgroundScope).also {
            it.showLayer(null)
            runCurrent()
        }
    }

    private fun AgentSettingsController.view(): AgentSettingsView = assertNotNull(state.value.view)

    private fun TestScope.announce(vararg changes: ApiEventBody) {
        changes.forEach { core.emit(appWide(it)) }
        runCurrent()
    }

    // ---- エージェント ----

    @Test
    fun readsTheSettingsTheModelsOfEveryHarnessAndTheEffortsTheListLacks() = runTest {
        val c = agent()
        val s = c.state.value
        assertEquals(core.agentRevision, c.view().revision)
        assertEquals(listOf("sonnet", "opus"), s.models.getValue("claude-code").models?.models?.map { it.value })
        // sonnet の effort は一覧にあるので読まない。opus のは 1 回だけ読む
        assertEquals(listOf(ApiCommand.ListModelEfforts("claude-code", "opus")), core.callsOf<ApiCommand.ListModelEfforts>())
        assertEquals(listOf("high"), s.efforts.getValue(modelKey("claude-code", "opus")).efforts?.map { it.value })
        assertFalse(s.loadingModels)
        assertNull(s.error)
    }

    @Test
    fun savesWithTheRevisionThatIsShownAndShowsTheAnswer() = runTest {
        val c = agent()
        val shown = c.view().revision
        val next = global.copy(candidates = global.candidates + Candidate("claude-code", "haiku", null, ""))
        c.save(AgentRole.Implementer, next)
        runCurrent()
        val sent = core.callsOf<ApiCommand.SetAgentSettings>().single()
        assertEquals(shown, sent.expectedRevision)
        assertEquals(AgentRole.Implementer, sent.role)
        assertEquals(next, sent.settings)
        assertEquals(shown + 1, c.view().revision)
        assertEquals(next, c.view().effective.implementer)
        assertFalse(c.state.value.saving)
        assertNull(c.state.value.error)
    }

    @Test
    fun savesAProjectsLayerWithoutTouchingTheGlobalOne() = runTest {
        val store = newStore(core)
        val c = AgentSettingsController(store.settings, backgroundScope)
        c.showLayer("repo")
        runCurrent()
        assertEquals("repo", c.view().project)
        val own = role(opus)
        c.save(AgentRole.Implementer, own)
        runCurrent()
        assertEquals("repo", core.callsOf<ApiCommand.SetAgentSettings>().single().project)
        assertEquals(own, c.view().projectLayer?.implementer)
        assertEquals(global, c.view().global.implementer)
        // 外すと全体の設定を使う
        c.save(AgentRole.Implementer, null)
        runCurrent()
        assertNull(c.view().projectLayer?.implementer)
        assertEquals(global, c.view().effective.implementer)
    }

    @Test
    fun anEditTheCoreRefusesIsShownAndNotSent() = runTest {
        val c = agent()
        c.apply(AgentRole.Implementer, updateRow(global, 1) { it.copy(model = "sonnet", effort = "medium") })
        runCurrent()
        assertEquals(DUPLICATE_ROW, c.state.value.error)
        assertEquals(emptyList(), core.callsOf<ApiCommand.SetAgentSettings>())
    }

    @Test
    fun doesNotOverwriteSettingsAnotherClientChangedTellsSoAndShowsTheLatest() = runTest {
        val c = agent()
        val shown = c.view().revision
        // 他の端末が先に保存した
        core.agentRevision += 1
        c.save(AgentRole.Implementer, role(opus))
        runCurrent()
        assertEquals(0, core.agentSettingsSaves)
        assertEquals(CONFLICT_MESSAGE, c.state.value.error)
        assertEquals(shown + 1, c.view().revision)
        // 読み直した版で保存し直せる
        c.save(AgentRole.Implementer, role(opus))
        runCurrent()
        assertEquals(1, core.agentSettingsSaves)
        assertNull(c.state.value.error)
    }

    @Test
    fun showsAnyOtherFailureOfASave() = runTest {
        val c = agent()
        core.fail(ApiCommand.SetAgentSettings::class, ApiErrorCode.InvalidArgument, "unknown harness: x")
        c.save(AgentRole.Implementer, role(opus))
        runCurrent()
        assertEquals("unknown harness: x", c.state.value.error)
        assertFalse(c.state.value.saving)
    }

    @Test
    fun readsAgainWhenAnotherClientChangedTheSettings() = runTest {
        val c = agent()
        val reads = core.callsOf<ApiCommand.GetAgentSettings>().size
        core.agentRevision += 5
        announce(ApiEventBody.SettingsChanged(SettingsKind.AgentSettings, null, core.agentRevision))
        assertEquals(reads + 1, core.callsOf<ApiCommand.GetAgentSettings>().size)
        assertEquals(core.agentRevision, c.view().revision)
        assertFalse(c.stale.value)
        // ハーネスの変更でも読み直す。秘密の環境変数は関係ない
        announce(ApiEventBody.SettingsChanged(SettingsKind.Harnesses))
        assertEquals(reads + 2, core.callsOf<ApiCommand.GetAgentSettings>().size)
        announce(ApiEventBody.SettingsChanged(SettingsKind.Secrets))
        assertEquals(reads + 2, core.callsOf<ApiCommand.GetAgentSettings>().size)
    }

    @Test
    fun ignoresAnAnnouncementOfARevisionThatIsAlreadyShownSuchAsItsOwnSave() = runTest {
        val c = agent()
        c.save(AgentRole.Implementer, role(opus))
        runCurrent()
        val reads = core.callsOf<ApiCommand.GetAgentSettings>().size
        announce(ApiEventBody.SettingsChanged(SettingsKind.AgentSettings, null, c.view().revision))
        announce(ApiEventBody.SettingsChanged(SettingsKind.AgentSettings, null, c.view().revision - 1))
        assertEquals(reads, core.callsOf<ApiCommand.GetAgentSettings>().size)
    }

    @Test
    fun readsAgainAfterAMissedChangeOfAnyKind() = runTest {
        val c = agent()
        val reads = core.callsOf<ApiCommand.GetAgentSettings>().size
        // 再接続・resync: 何が変わったか分からない
        announceMissed()
        assertEquals(reads + 1, core.callsOf<ApiCommand.GetAgentSettings>().size)
        assertNotNull(c.state.value.view)
    }

    private fun TestScope.announceMissed() {
        core.emit(appWide(ApiEventBody.Resync))
        runCurrent()
    }

    @Test
    fun whileATextIsBeingTypedTheChangeWaitsBehindAReloadBanner() = runTest {
        val c = agent()
        val reads = core.callsOf<ApiCommand.GetAgentSettings>().size
        c.editing.set("note-0", true)
        core.agentRevision += 1
        announce(ApiEventBody.SettingsChanged(SettingsKind.AgentSettings, null, core.agentRevision))
        assertEquals(reads, core.callsOf<ApiCommand.GetAgentSettings>().size)
        assertTrue(c.stale.value)
        // 「再読込」
        c.reloadNow()
        runCurrent()
        assertFalse(c.stale.value)
        assertEquals(core.agentRevision, c.view().revision)
    }

    @Test
    fun aChangeThatArrivesDuringASaveIsLookedAtWhenTheSaveIsDone() = runTest {
        val c = agent()
        core.before = { if (it is ApiCommand.SetAgentSettings) core.emit(appWide(ApiEventBody.SettingsChanged(SettingsKind.AgentSettings, null, 9_999))) }
        c.save(AgentRole.Implementer, role(opus))
        runCurrent()
        // 自分の保存の知らせではない (版が先): 保存が終わってから読み直す
        assertEquals(2, core.callsOf<ApiCommand.GetAgentSettings>().size)
        assertFalse(c.stale.value)
    }

    @Test
    fun switchingTheLayerReadsThatLayerAndDropsTheOldView() = runTest {
        val c = agent()
        c.showLayer("repo")
        assertNull(c.state.value.view)
        runCurrent()
        assertEquals("repo", c.view().project)
        assertEquals(listOf(null, "repo"), core.callsOf<ApiCommand.GetAgentSettings>().map { it.project })
    }

    @Test
    fun refreshingModelsAsksEveryHarnessAgainAndReadsTheEffortsAgain() = runTest {
        val c = agent()
        c.refreshModels()
        runCurrent()
        assertEquals(listOf(false, true), core.callsOf<ApiCommand.ListHarnessModels>().map { it.refresh ?: false })
        assertEquals(2, core.callsOf<ApiCommand.ListModelEfforts>().size)
    }

    @Test
    fun aHarnessWhoseModelsCannotBeReadIsShownWithItsReason() = runTest {
        core.harnessModels.clear()
        val c = agent()
        val models = c.state.value.models.getValue("claude-code")
        assertNull(models.models)
        assertEquals("no harness claude-code", models.error)
    }

    // ---- ハーネス ----

    private fun TestScope.harness(): HarnessSettingsController {
        val store = newStore(core)
        return HarnessSettingsController(store.settings, backgroundScope).also {
            it.reload()
            runCurrent()
        }
    }

    @Test
    fun readsTheHarnessesAndFillsTheDraftsWithTheSavedPaths() = runTest {
        val c = harness()
        assertEquals(listOf("claude-code", "devin"), c.state.value.harnesses?.map { it.id })
        assertEquals(mapOf("claude-code" to "", "devin" to ""), draftsOf(c.state.value.harnesses.orEmpty()))
        assertEquals(1, c.state.value.epoch)
    }

    @Test
    fun savesAManualPathAndRemovesIt() = runTest {
        val c = harness()
        assertTrue(c.setPath("devin", "/opt/bin/devin"))
        assertEquals("/opt/bin/devin", c.state.value.harnesses?.single { it.id == "devin" }?.overridePath)
        assertTrue(c.setPath("devin", null))
        assertNull(c.state.value.harnesses?.single { it.id == "devin" }?.overridePath)
        assertEquals(listOf("/opt/bin/devin", null), core.callsOf<ApiCommand.SetHarnessPath>().map { it.path })
    }

    @Test
    fun aRefusedPathIsShownOnItsRowOnly() = runTest {
        val c = harness()
        core.fail(ApiCommand.SetHarnessPath::class, ApiErrorCode.InvalidArgument, "not an executable file")
        assertFalse(c.setPath("devin", "/etc/passwd"))
        assertEquals(mapOf("devin" to "not an executable file"), c.state.value.rowErrors)
        assertNull(c.state.value.error)
        assertFalse(c.state.value.busy)
    }

    @Test
    fun detectingAgainAsksTheCoreToForgetAndKeepsWhatWasTyped() = runTest {
        val c = harness()
        val epoch = c.state.value.epoch
        c.detect()
        runCurrent()
        assertEquals(1, core.callsOf<ApiCommand.DetectHarnesses>().size)
        // 最初の読み取りが成功していれば入力欄は触らない
        assertEquals(epoch, c.state.value.epoch)
        assertFalse(c.state.value.detecting)
    }

    @Test
    fun aFailedFirstReadIsShownAndDetectingFillsTheDrafts() = runTest {
        core.fail(ApiCommand.GetHarnesses::class, ApiErrorCode.Unavailable, "no harness registry")
        val c = harness()
        assertEquals("no harness registry", c.state.value.error)
        assertNull(c.state.value.harnesses)
        c.detect()
        runCurrent()
        assertEquals(2, c.state.value.harnesses?.size)
        assertEquals(1, c.state.value.epoch)
        assertNull(c.state.value.error)
    }

    @Test
    fun harnessChangesReadAgainUnlessAPathIsUnsavedThenABannerOffersIt() = runTest {
        val c = harness()
        val reads = core.callsOf<ApiCommand.GetHarnesses>().size
        announce(ApiEventBody.SettingsChanged(SettingsKind.Harnesses))
        assertEquals(reads + 1, core.callsOf<ApiCommand.GetHarnesses>().size)
        // 保存していないパスがある
        c.unsaved = true
        announce(ApiEventBody.SettingsChanged(SettingsKind.Harnesses))
        assertEquals(reads + 1, core.callsOf<ApiCommand.GetHarnesses>().size)
        assertTrue(c.stale.value)
        c.unsaved = false
        c.reloadNow()
        runCurrent()
        assertEquals(reads + 2, core.callsOf<ApiCommand.GetHarnesses>().size)
        assertFalse(c.stale.value)
        // エージェント設定の変更は関係ない
        announce(ApiEventBody.SettingsChanged(SettingsKind.AgentSettings, null, 1))
        assertEquals(reads + 2, core.callsOf<ApiCommand.GetHarnesses>().size)
    }

    // ---- 秘密の環境変数 ----

    private fun TestScope.secrets(): SecretEnvController {
        val store = newStore(core)
        return SecretEnvController(store.settings, backgroundScope).also {
            it.reload()
            runCurrent()
        }
    }

    @Test
    fun listsNamesRegistersAndDeletesWithoutKeepingTheValue() = runTest {
        core.secrets += "STRIPE_TEST_KEY"
        val c = secrets()
        assertEquals(listOf("STRIPE_TEST_KEY"), c.state.value.names)
        assertEquals(RegisterResult.Saved, c.register("  OPENROUTER_API_KEY ", "sk-secret"))
        assertEquals(listOf("STRIPE_TEST_KEY", "OPENROUTER_API_KEY"), c.state.value.names)
        assertEquals(ApiCommand.SetSecretEnv("OPENROUTER_API_KEY", "sk-secret"), core.callsOf<ApiCommand.SetSecretEnv>().single())
        // 状態のどこにも値は残らない
        assertFalse(c.state.value.toString().contains("sk-secret"))
        c.remove("STRIPE_TEST_KEY")
        assertEquals(listOf("OPENROUTER_API_KEY"), c.state.value.names)
    }

    @Test
    fun refusesABadNameOrAnEmptyValueBeforeSendingAnything() = runTest {
        val c = secrets()
        assertEquals(RegisterResult.Invalid, c.register("", "v"))
        assertEquals("名前を入力してください", c.state.value.error)
        assertEquals(RegisterResult.Invalid, c.register("1BAD", "v"))
        assertEquals("名前は英数字と _ だけで、数字から始められません", c.state.value.error)
        assertEquals(RegisterResult.Invalid, c.register("GOOD", ""))
        assertEquals("値を入力してください", c.state.value.error)
        assertEquals(emptyList(), core.callsOf<ApiCommand.SetSecretEnv>())
        // 直したら誤りの表示は消える
        assertEquals(RegisterResult.Saved, c.register("GOOD", "v"))
        assertNull(c.state.value.error)
    }

    @Test
    fun aFailedRegistrationShowsTheCoresReason() = runTest {
        val c = secrets()
        core.fail(ApiCommand.SetSecretEnv::class, ApiErrorCode.Unavailable, "no Secret Service")
        assertEquals(RegisterResult.Failed, c.register("GOOD", "v"))
        assertEquals("no Secret Service", c.state.value.error)
        assertFalse(c.state.value.busy)
    }

    @Test
    fun secretChangesReadAgainUnlessTheFormIsBeingFilledThenABannerOffersIt() = runTest {
        val c = secrets()
        core.secrets += "NEW_FROM_PC"
        announce(ApiEventBody.SettingsChanged(SettingsKind.Secrets))
        assertEquals(listOf("NEW_FROM_PC"), c.state.value.names)
        c.editing.set("form", true)
        core.secrets += "ANOTHER"
        announce(ApiEventBody.SettingsChanged(SettingsKind.Secrets))
        assertEquals(listOf("NEW_FROM_PC"), c.state.value.names)
        assertTrue(c.stale.value)
        c.editing.set("form", false)
        c.reloadNow()
        runCurrent()
        assertEquals(listOf("NEW_FROM_PC", "ANOTHER"), c.state.value.names)
    }
}
