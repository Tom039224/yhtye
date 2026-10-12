package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentChoice
import io.github.tom039224.yhtye.shared.api.generated.Candidate
import io.github.tom039224.yhtye.shared.api.generated.EffortOption
import io.github.tom039224.yhtye.shared.api.generated.HarnessInfo
import io.github.tom039224.yhtye.shared.api.generated.HarnessModels
import io.github.tom039224.yhtye.shared.api.generated.ModelOption
import io.github.tom039224.yhtye.shared.api.generated.RoleSettings
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** PC の `agentSettings.ts` と各節の検査の移植 (行の編集・モデルの検索・名前の検査)。 */
class AgentSettingsEditTest {
    private val sonnet = Candidate("claude-code", "sonnet", "medium", "ふつう")
    private val opus = Candidate("claude-code", "opus", "high", "難しい")
    private val settings = RoleSettings(listOf(sonnet, opus), default = rowChoice(sonnet))

    private fun ok(edit: Edit): RoleSettings = assertIs<Edit.Ok>(edit).settings

    @Test
    fun changingTheDefaultRowKeepsTheDefaultOnIt() {
        val next = ok(updateRow(settings, 0) { it.copy(model = "haiku", effort = null) })
        assertEquals(AgentChoice("claude-code", "haiku", null), next.default)
        assertEquals("haiku", next.candidates[0].model)
        assertEquals(opus, next.candidates[1])
    }

    @Test
    fun changingAnotherRowKeepsTheDefault() {
        val next = ok(updateRow(settings, 1) { it.copy(note = "もっと難しい") })
        assertEquals(settings.default, next.default)
        assertEquals("もっと難しい", next.candidates[1].note)
    }

    @Test
    fun refusesARowThatDuplicatesAnotherHarnessModelAndEffort() {
        val refused = updateRow(settings, 1) { it.copy(model = "sonnet", effort = "medium") }
        assertEquals(Edit.Refused(DUPLICATE_ROW), refused)
        // メモだけが違う行も重複 (ハーネス × モデル × effort で見る)
        assertEquals(Edit.Refused(DUPLICATE_ROW), addRow(settings, sonnet.copy(note = "別のメモ")))
        // effort が違えば同じモデルでもよい
        assertTrue(addRow(settings, sonnet.copy(effort = "high")) is Edit.Ok)
    }

    @Test
    fun refusesToChangeARowThatIsNotThere() {
        assertEquals(Edit.Refused("行が見つかりません"), updateRow(settings, 5) { it })
    }

    @Test
    fun removingTheDefaultMakesTheFirstRemainingRowTheDefault() {
        val next = removeRow(settings, 0)!!
        assertEquals(listOf(opus), next.candidates)
        assertEquals(rowChoice(opus), next.default)
        // 既定でない行を消しても既定はそのまま
        assertEquals(settings.default, removeRow(settings, 1)!!.default)
    }

    @Test
    fun theLastRowCannotBeRemoved() {
        val only = RoleSettings(listOf(sonnet), rowChoice(sonnet))
        assertNull(removeRow(only, 0))
        assertNull(removeRow(settings, 7))
    }

    @Test
    fun makesARowTheDefault() {
        assertEquals(rowChoice(opus), withDefaultRow(settings, 1).default)
        assertEquals(settings, withDefaultRow(settings, 9))
    }

    private val harnesses = listOf(HarnessInfo("claude-code", "Claude Code", false), HarnessInfo("opencode", "OpenCode", true))

    private fun models(harness: String, vararg values: String) =
        ModelsState(HarnessModels(harness, values.map { ModelOption(it, it.uppercase()) }, null, 0))

    @Test
    fun aNewRowIsTheFirstUnusedModelWithoutEffortStartingWithTheLastRowsHarness() {
        val loaded = mapOf("claude-code" to models("claude-code", "sonnet", "opus", "haiku"), "opencode" to models("opencode", "x/y"))
        // sonnet/medium と opus/high はあるが、effort なしの組み合わせはまだ無い
        assertEquals(Candidate("claude-code", "sonnet", null, ""), newRow(harnesses, loaded, settings))
        val plain = settings.copy(candidates = settings.candidates + Candidate("claude-code", "sonnet", null, ""))
        assertEquals(Candidate("claude-code", "opus", null, ""), newRow(harnesses, loaded, plain))
        // 最後の行のハーネスが先 (最後が opencode なら opencode の残りから)
        val lastOpencode = RoleSettings(listOf(sonnet, Candidate("opencode", "x/z", null, "")), rowChoice(sonnet))
        assertEquals("opencode", newRow(harnesses, loaded, lastOpencode)?.harness)
        // 全部使っていたら null
        val used = RoleSettings(
            listOf(Candidate("claude-code", "sonnet", null, ""), Candidate("opencode", "x/y", null, "")),
            rowChoice(sonnet),
        )
        assertNull(newRow(harnesses, mapOf("claude-code" to models("claude-code", "sonnet"), "opencode" to models("opencode", "x/y")), used))
    }

    @Test
    fun aNewRowNeedsTheModelListsToBeLoadedAndHarnessesWithoutModelsGetTheDefaultModel() {
        assertNull(newRow(harnesses, emptyMap(), settings))
        val none = mapOf("claude-code" to ModelsState(HarnessModels("claude-code", emptyList(), null, 0)))
        assertEquals(Candidate("claude-code", null, null, ""), newRow(harnesses.take(1), none, RoleSettings(listOf(opus), rowChoice(opus))))
        // モデルが要るハーネスの空の一覧では選べない
        val required = mapOf("opencode" to ModelsState(HarnessModels("opencode", emptyList(), null, 0)))
        assertNull(newRow(harnesses.drop(1), required, RoleSettings(listOf(opus), rowChoice(opus))))
    }

    @Test
    fun theFirstModelAfterChangingTheHarness() {
        val h = harnesses[1]
        assertEquals(FirstModel.NoneAvailable, firstModel(h, null))
        assertEquals(FirstModel.NoneAvailable, firstModel(h, HarnessModels("opencode", emptyList(), null, 0)))
        assertEquals(FirstModel.Of(null), firstModel(harnesses[0], HarnessModels("claude-code", emptyList(), null, 0)))
        assertEquals(FirstModel.Of("a"), firstModel(h, HarnessModels("opencode", listOf(ModelOption("a", "A")), null, 0)))
    }

    @Test
    fun searchesLongModelListsByAllWordsGroupedByProviderWithAnUpperLimit() {
        val many = HarnessModels(
            "opencode",
            (1..60).map { ModelOption("p/model-$it", "Model $it") } + ModelOption("q/free-gpt", "Free GPT") + ModelOption("plain", "Plain"),
            null,
            0,
        )
        assertEquals(PickerResult(emptyList(), 0), pickerModels(many, "  "))
        val gpt = pickerModels(many, "free gpt")
        assertEquals(listOf("q"), gpt.groups.map { it.provider })
        assertEquals(0, gpt.more)
        val broad = pickerModels(many, "model")
        assertEquals(MAX_MATCHES, broad.groups.sumOf { it.models.size })
        assertEquals(60 - MAX_MATCHES, broad.more)
        assertEquals(listOf(null), pickerModels(many, "PLAIN").groups.map { it.provider })
        assertTrue(isLongList(many))
        assertTrue(!isLongList(HarnessModels("h", many.models.take(LONG_LIST), null, 0)))
    }

    @Test
    fun knowsTheEffortsFromTheListOrTheCache() {
        val listed = HarnessModels(
            "h",
            listOf(ModelOption("a", "A", efforts = listOf(EffortOption("low", "Low"))), ModelOption("b", "B")),
            null,
            0,
        )
        val cache = mapOf(modelKey("h", "b") to EffortsState(listOf(EffortOption("high", "High"))))
        assertEquals(listOf("low"), knownEfforts(listed, cache, "h", "a")?.map { it.value })
        assertEquals(listOf("high"), knownEfforts(listed, cache, "h", "b")?.map { it.value })
        assertNull(knownEfforts(listed, emptyMap(), "h", "b"))
        assertEquals(emptyList(), knownEfforts(listed, emptyMap(), "h", null))
    }

    @Test
    fun labelsChoicesLikeThePcDoes() {
        val listed = HarnessModels("claude-code", listOf(ModelOption("sonnet", "Sonnet"), ModelOption("same", "same")), null, 0)
        assertEquals("Sonnet (sonnet)", modelText("sonnet", listed))
        assertEquals("same", modelText("same", listed))
        assertEquals("unknown", modelText("unknown", listed))
        assertEquals("Claude Code · Sonnet (sonnet) · effort high", choiceLabel(AgentChoice("claude-code", "sonnet", "high"), harnesses, listed))
        assertEquals("gone · 既定のモデル", choiceLabel(AgentChoice("gone", null, null), harnesses))
        assertEquals("High (high)", effortText(EffortOption("high", "High")))
        assertEquals("high", effortText(EffortOption("high", "high")))
    }

    @Test
    fun aRowOfAnUninstalledHarnessIsMissing() {
        assertTrue(isMissing(AgentChoice("gone", null, null), harnesses))
        assertTrue(!isMissing(sonnet, harnesses))
    }

    @Test
    fun checksTheNamesOfSecretVariables() {
        assertNull(secretNameError("OPENROUTER_API_KEY_CODEX"))
        assertNull(secretNameError("_x1"))
        assertEquals("名前を入力してください", secretNameError(""))
        assertEquals("名前は英数字と _ だけで、数字から始められません", secretNameError("1ABC"))
        assertEquals("名前は英数字と _ だけで、数字から始められません", secretNameError("A-B"))
        assertEquals("名前は 128 文字までです", secretNameError("A".repeat(129)))
        assertNull(secretNameError("A".repeat(128)))
    }
}
