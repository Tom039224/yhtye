package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentChoice
import io.github.tom039224.yhtye.shared.api.generated.AgentRole
import io.github.tom039224.yhtye.shared.api.generated.Candidate
import io.github.tom039224.yhtye.shared.api.generated.EffortOption
import io.github.tom039224.yhtye.shared.api.generated.HarnessInfo
import io.github.tom039224.yhtye.shared.api.generated.HarnessModels
import io.github.tom039224.yhtye.shared.api.generated.HarnessPathSource
import io.github.tom039224.yhtye.shared.api.generated.ModelOption
import io.github.tom039224.yhtye.shared.api.generated.RoleSettings

// エージェント設定の純粋な部分 (PC の `src/ui/agentSettings.ts` と `HarnessSettingsSection.tsx` / `SecretEnvSection.tsx` の
// 検査の移植): 役割、役割ごとの候補の行 (ハーネス × モデル × effort + 用途メモ、うち 1 つが既定) とその編集。
// 文言・規則は PC と同じ。

/** 設定画面に出す役割 (PC の `ROLES`)。 */
data class RoleEntry(val role: AgentRole, val label: String, val hint: String)

val ROLES: List<RoleEntry> = listOf(
    RoleEntry(AgentRole.Orchestrator, "オーケストレータ", "あなたと話し、タスクを組む"),
    RoleEntry(AgentRole.Implementer, "実装", "code タスク"),
    RoleEntry(AgentRole.Investigator, "調査", "investigate タスク"),
    RoleEntry(AgentRole.Reviewer, "レビュー", "review Step"),
)

/** コアが受け付ける用途メモの最大の文字数。 */
const val MAX_NOTE_CHARS: Int = 400

/** あるハーネスのモデルについて分かっていること。 */
data class ModelsState(val models: HarnessModels? = null, val error: String? = null, val loading: Boolean = false)

/** 一覧に effort が無かったモデルの effort について分かっていること。 */
data class EffortsState(val efforts: List<EffortOption>? = null, val error: String? = null, val loading: Boolean = false)

/** `(ハーネス, モデル)` ごとの effort の覚えの鍵。 */
fun modelKey(harness: String, model: String): String = "$harness/$model"

/** 行のハーネス × モデル × effort (用途メモなし)。 */
fun rowChoice(c: Candidate): AgentChoice = AgentChoice(c.harness, c.model, c.effort)

fun sameChoice(a: AgentChoice, b: AgentChoice): Boolean = a.harness == b.harness && a.model == b.model && a.effort == b.effort

private fun sameChoice(a: Candidate, b: AgentChoice): Boolean = sameChoice(rowChoice(a), b)

/** `名前 (値)`。ハーネスが別の名前を出さないなら値だけ。 */
fun modelText(model: String, listed: HarnessModels?): String {
    val name = listed?.models?.find { it.value == model }?.name
    return if (name != null && name != model) "$name ($model)" else model
}

/** `Claude Code · Sonnet (sonnet) · effort high` (文の中に出す)。 */
fun choiceLabel(c: AgentChoice, harnesses: List<HarnessInfo>, models: HarnessModels? = null): String {
    val harness = harnesses.find { it.id == c.harness }?.label ?: c.harness
    val model = if (c.model == null) "既定のモデル" else modelText(c.model, models)
    return "$harness · $model${if (c.effort != null) " · effort ${c.effort}" else ""}"
}

/** 編集の結果: 新しい設定、またはできない理由。 */
sealed interface Edit {
    data class Ok(val settings: RoleSettings) : Edit

    data class Refused(val message: String) : Edit
}

const val DUPLICATE_ROW: String = "同じハーネス・モデル・effort の行が既にあります (行ごとに組み合わせを変えてください)"

private fun hasTriple(rows: List<Candidate>, choice: AgentChoice, except: Int = -1): Boolean =
    rows.withIndex().any { (i, r) -> i != except && sameChoice(r, choice) }

/**
 * 行 [index] を [patch] で変えた設定。既定の行だったら既定も付いていく。ほかの行と同じハーネス × モデル × effort に
 * なる変更は断る。
 */
fun updateRow(settings: RoleSettings, index: Int, patch: (Candidate) -> Candidate): Edit {
    val old = settings.candidates.getOrNull(index) ?: return Edit.Refused("行が見つかりません")
    val row = patch(old)
    if (hasTriple(settings.candidates, rowChoice(row), index)) return Edit.Refused(DUPLICATE_ROW)
    val candidates = settings.candidates.mapIndexed { i, r -> if (i == index) row else r }
    val wasDefault = sameChoice(old, settings.default)
    return Edit.Ok(RoleSettings(candidates, if (wasDefault) rowChoice(row) else settings.default))
}

/** [row] を足した設定 (同じ組み合わせがあれば断る)。 */
fun addRow(settings: RoleSettings, row: Candidate): Edit {
    if (hasTriple(settings.candidates, rowChoice(row))) return Edit.Refused(DUPLICATE_ROW)
    return Edit.Ok(settings.copy(candidates = settings.candidates + row))
}

/**
 * 行 [index] を除いた設定。既定を消したら残りの先頭が既定になる。最後の 1 行は消せない (null)。
 */
fun removeRow(settings: RoleSettings, index: Int): RoleSettings? {
    val candidates = settings.candidates.filterIndexed { i, _ -> i != index }
    if (candidates.isEmpty() || candidates.size == settings.candidates.size) return null
    val keepDefault = candidates.any { sameChoice(it, settings.default) }
    return RoleSettings(candidates, if (keepDefault) settings.default else rowChoice(candidates[0]))
}

/** 行 [index] を既定にした設定。 */
fun withDefaultRow(settings: RoleSettings, index: Int): RoleSettings {
    val row = settings.candidates.getOrNull(index) ?: return settings
    return settings.copy(default = rowChoice(row))
}

/** モデルがこれより多いハーネスは、選択肢の代わりに検索つきの選び方にする。 */
const val LONG_LIST: Int = 12

/** 検索で出す最大の件数 (残りは検索語を絞ってもらう)。 */
const val MAX_MATCHES: Int = 40

fun isLongList(listed: HarnessModels?): Boolean = (listed?.models?.size ?: 0) > LONG_LIST

/** 空白で区切った語がすべてモデルの名前か値に含まれるか。 */
fun matchesModel(model: ModelOption, query: String): Boolean {
    val hay = "${model.name} ${model.value}".lowercase()
    return query.lowercase().split(Regex("\\s+")).filter { it.isNotEmpty() }.all { hay.contains(it) }
}

/** 見出し (モデルの値の `<provider>/` の前) とそのモデル。 */
data class ModelGroup(val provider: String?, val models: List<ModelOption>)

/** @property more 出さなかった一致の数 ([MAX_MATCHES] 超え)。 */
data class PickerResult(val groups: List<ModelGroup>, val more: Int)

/**
 * 検索つきの選び方に出すモデル: 語がすべて一致するもの (空の検索語では何も出さない: 何百ものモデルは検索するだけ)、
 * 最大 [MAX_MATCHES] 件を提供元ごとに。
 */
fun pickerModels(listed: HarnessModels, query: String): PickerResult {
    if (query.isBlank()) return PickerResult(emptyList(), 0)
    val matched = listed.models.filter { matchesModel(it, query) }
    val groups = LinkedHashMap<String?, MutableList<ModelOption>>()
    for (m in matched.take(MAX_MATCHES)) {
        val provider = if (m.value.contains('/')) m.value.substringBefore('/') else null
        groups.getOrPut(provider) { mutableListOf() } += m
    }
    return PickerResult(groups.map { (p, ms) -> ModelGroup(p, ms) }, maxOf(0, matched.size - MAX_MATCHES))
}

/** [harness] の [model] について分かっている effort。まだ分からなければ null。 */
fun knownEfforts(
    listed: HarnessModels?,
    cache: Map<String, EffortsState>,
    harness: String,
    model: String?,
): List<EffortOption>? {
    if (model == null) return emptyList()
    val fromList = listed?.models?.find { it.value == model }?.efforts
    if (fromList != null) return fromList
    return cache[modelKey(harness, model)]?.efforts
}

/**
 * 新しい行: どの行にもまだ無い最初のハーネス × モデル (effort なし)。最後の行のハーネスから探す。残っていない・
 * 一覧を読めていなければ null。
 */
fun newRow(harnesses: List<HarnessInfo>, models: Map<String, ModelsState>, settings: RoleSettings): Candidate? {
    val last = settings.candidates.lastOrNull()?.harness
    val ordered = harnesses.sortedBy { if (it.id == last) 0 else 1 }
    for (h in ordered) {
        val listed = models[h.id]?.models ?: continue
        val values: List<String?> = when {
            listed.models.isNotEmpty() -> listed.models.map { it.value }
            h.requiresModel -> emptyList()
            else -> listOf(null)
        }
        for (model in values) {
            val row = Candidate(harness = h.id, model = model, effort = null, note = "")
            if (!hasTriple(settings.candidates, rowChoice(row))) return row
        }
    }
    return null
}

/** ハーネスを替えた行が最初に使うモデル。`Some(null)` = 既定のモデル、null = 選べない。 */
sealed interface FirstModel {
    data class Of(val model: String?) : FirstModel

    data object NoneAvailable : FirstModel
}

fun firstModel(harness: HarnessInfo, listed: HarnessModels?): FirstModel = when {
    listed == null -> FirstModel.NoneAvailable
    listed.models.isNotEmpty() -> FirstModel.Of(listed.models[0].value)
    harness.requiresModel -> FirstModel.NoneAvailable
    else -> FirstModel.Of(null)
}

/** 行のハーネスが登録されていない (例: OpenCode がもう入っていない)。 */
fun isMissing(choice: AgentChoice, harnesses: List<HarnessInfo>): Boolean = harnesses.none { it.id == choice.harness }

fun isMissing(row: Candidate, harnesses: List<HarnessInfo>): Boolean = harnesses.none { it.id == row.harness }

/** effort の表示 (`名前 (値)`、別の名前が無ければ値だけ)。 */
fun effortText(e: EffortOption): String = if (e.name.isNotEmpty() && e.name != e.value) "${e.name} (${e.value})" else e.value

/** ハーネスの主実行ファイルの見つかった場所の表示 (PC の `PATH_SOURCE_LABELS`)。 */
val PATH_SOURCE_LABELS: Map<HarnessPathSource, String> = mapOf(
    HarnessPathSource.Override to "手動",
    HarnessPathSource.Path to "PATH",
    HarnessPathSource.KnownDir to "既知の場所",
    HarnessPathSource.None to "—",
)

/** 秘密の環境変数の名前の最大の文字数 (コアも同じ検査をする)。 */
const val MAX_SECRET_NAME: Int = 128
private val SECRET_NAME = Regex("^[A-Za-z_][A-Za-z0-9_]*$")

/** [name] が環境変数の名前になれない理由 (なれるなら null)。 */
fun secretNameError(name: String): String? = when {
    name.isEmpty() -> "名前を入力してください"
    name.length > MAX_SECRET_NAME -> "名前は $MAX_SECRET_NAME 文字までです"
    !SECRET_NAME.matches(name) -> "名前は英数字と _ だけで、数字から始められません"
    else -> null
}
