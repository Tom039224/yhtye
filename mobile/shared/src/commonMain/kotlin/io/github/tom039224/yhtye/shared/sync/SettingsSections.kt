package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentRole
import io.github.tom039224.yhtye.shared.api.generated.AgentSettingsView
import io.github.tom039224.yhtye.shared.api.generated.Candidate
import io.github.tom039224.yhtye.shared.api.generated.HarnessDetection
import io.github.tom039224.yhtye.shared.api.generated.RoleSettings
import io.github.tom039224.yhtye.shared.api.generated.SettingsKind
import io.github.tom039224.yhtye.shared.client.RpcException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

// 設定画面の 3 つの節 (エージェント / ハーネス / 秘密の環境変数) の状態と操作。PC の `AgentSettingsSection.tsx` /
// `HarnessSettingsSection.tsx` / `SecretEnvSection.tsx` と `useSettingsChanges.ts` の写し (T-65 の仕様: 他の端末の変更は
// 読み直し、編集中ならバナー、エージェント設定は表示した版を付けて保存し `conflict` なら読み直す)。
//
// 状態の変更は [scope] (画面のメインスレッド) でだけ行う。入力欄の中身は画面が持つ (IME の途中の文字を壊さないため)ので、
// ここは「編集中か」だけを [SettingsEditing] で教えてもらう。

/** 画面が教える、読み直しで捨てたくない編集中の入力 (欄の名前の集合)。 */
class SettingsEditing {
    private val keys = HashSet<String>()

    val isEditing: Boolean get() = keys.isNotEmpty()

    fun set(key: String, editing: Boolean) {
        if (editing) keys += key else keys -= key
    }
}

/**
 * [kinds] の設定が (どの端末でも) 変わったら読み直す。編集中なら読み直さず [stale] を立てる (画面が「再読込」のバナーを出す)。
 * 保存中に届いた変更は保存が終わるまで待つ (自分の保存の知らせかもしれない)。取りこぼし (`kind == null`) はどの種類でも数える。
 */
class SettingsChangeGate(
    scope: CoroutineScope,
    changes: Flow<SettingsChange>,
    private val kinds: Set<SettingsKind>,
    private val reload: () -> Unit,
    private val isBusy: () -> Boolean,
    private val isEditing: () -> Boolean,
    /** 画面にもう出ている変更 (自分の保存など)。 */
    private val isShown: (SettingsChange) -> Boolean = { false },
) {
    private val _stale = MutableStateFlow(false)
    val stale: StateFlow<Boolean> = _stale.asStateFlow()
    private var pending: SettingsChange? = null

    init {
        // すぐ購読する (始める前に届いた知らせは取りこぼさないが、始めた後のものは落とさない)
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            changes.collect { change -> if (change.kind == null || change.kind in kinds) consider(change) }
        }
    }

    private fun consider(change: SettingsChange) {
        when {
            isShown(change) -> Unit
            isBusy() -> pending = change
            isEditing() -> _stale.value = true
            else -> reload()
        }
    }

    /** 保存が終わった: 待たせた変更を見る。 */
    fun resume() {
        val change = pending ?: return
        pending = null
        consider(change)
    }

    /** 「再読込」: 今すぐ読み直す。 */
    fun reloadNow() {
        _stale.value = false
        reload()
    }
}

const val CONFLICT_MESSAGE: String =
    "他の端末で設定が変更されていたため、この変更は保存していません。最新の設定を読み込み直しました。"

// ---- エージェント -------------------------------------------------------------------------------------

data class AgentSettingsState(
    val view: AgentSettingsView? = null,
    val models: Map<String, ModelsState> = emptyMap(),
    val efforts: Map<String, EffortsState> = emptyMap(),
    val error: String? = null,
    val saving: Boolean = false,
) {
    val loadingModels: Boolean get() = models.values.any { it.loading }
}

/**
 * 「エージェント」の節: 役割ごとの候補の行 (ハーネス × モデル × effort + 用途メモ) と既定の行。全体かプロジェクトの層
 * ([project])。モデルと effort はコアがハーネスから読む。変更はすぐ保存し、これから起動するエージェントから有効。
 */
class AgentSettingsController(
    private val commands: SettingsCommands,
    private val scope: CoroutineScope,
    val editing: SettingsEditing = SettingsEditing(),
) {
    private val _state = MutableStateFlow(AgentSettingsState())
    val state: StateFlow<AgentSettingsState> = _state.asStateFlow()

    /** 見ている層: null = 全体、それ以外 = そのプロジェクト。 */
    private var project: String? = null

    /** 読むたびに増える。古い答えが新しい答えを置き換えないため。 */
    private var reads = 0

    private val gate = SettingsChangeGate(
        scope = scope,
        changes = commands.changes,
        kinds = setOf(SettingsKind.AgentSettings, SettingsKind.Harnesses),
        reload = ::reload,
        isBusy = { _state.value.saving },
        isEditing = { editing.isEditing },
        // 版は増えるだけ: 表示している版より新しくない知らせは (自分の保存も) もう出ている
        isShown = { c ->
            c.kind == SettingsKind.AgentSettings && c.revision != null && (_state.value.view?.revision ?: -1L) >= c.revision
        },
    )

    val stale: StateFlow<Boolean> get() = gate.stale

    /** 見る層を替える ([project] が null なら全体)。表示を空にして読み直す。 */
    fun showLayer(project: String?) {
        this.project = project
        reads += 1
        _state.update { it.copy(view = null, error = null) }
        reload()
    }

    fun reload() {
        val read = ++reads
        val layer = project
        scope.launch {
            try {
                val view = commands.getAgentSettings(layer)
                if (read != reads) return@launch
                _state.update { it.copy(view = view) }
                ensureLoaded()
            } catch (e: RpcException) {
                if (read == reads) _state.update { it.copy(error = describe(e)) }
            }
        }
    }

    fun reloadNow() = gate.reloadNow()

    fun clearError() = _state.update { it.copy(error = null) }

    fun fail(message: String) = _state.update { it.copy(error = message) }

    /** 全ハーネスのモデル一覧を読み直す (コアにハーネスへ聞き直させる)。effort も読み直す。 */
    fun refreshModels() {
        _state.update { it.copy(efforts = emptyMap()) }
        _state.value.view?.harnesses?.forEach { loadModels(it.id, refresh = true) }
    }

    private fun loadModels(harness: String, refresh: Boolean) {
        _state.update { s ->
            s.copy(models = s.models + (harness to ModelsState(s.models[harness]?.models, null, loading = true)))
        }
        scope.launch {
            val next = try {
                ModelsState(commands.listHarnessModels(harness, refresh), null, loading = false)
            } catch (e: RpcException) {
                ModelsState(_state.value.models[harness]?.models, describe(e), loading = false)
            }
            _state.update { it.copy(models = it.models + (harness to next)) }
            ensureLoaded()
        }
    }

    /** 見えているハーネスのモデルと、一覧に effort が無かったモデルの effort を (まだなら) 読む。 */
    private fun ensureLoaded() {
        val s = _state.value
        val view = s.view ?: return
        view.harnesses.filter { it.id !in s.models }.forEach { loadModels(it.id, refresh = false) }
        val wanted = ROLES.flatMap { r -> view.effective.of(r.role).candidates }.mapNotNull { needsEfforts(it, s) }.distinct()
        wanted.forEach { (harness, model) -> loadEfforts(harness, model) }
    }

    private fun needsEfforts(c: Candidate, s: AgentSettingsState): Pair<String, String>? {
        val listed = s.models[c.harness]?.models ?: return null
        val model = c.model ?: return null
        if (knownEfforts(listed, s.efforts, c.harness, model) != null) return null
        return if (modelKey(c.harness, model) in s.efforts) null else c.harness to model
    }

    private fun loadEfforts(harness: String, model: String) {
        val key = modelKey(harness, model)
        _state.update { it.copy(efforts = it.efforts + (key to EffortsState(null, null, loading = true))) }
        scope.launch {
            val next = try {
                EffortsState(commands.listModelEfforts(harness, model).efforts, null, loading = false)
            } catch (e: RpcException) {
                EffortsState(null, describe(e), loading = false)
            }
            _state.update { it.copy(efforts = it.efforts + (key to next)) }
        }
    }

    /** [role] の設定を [settings] に置き換える (null: 外して継承に戻す)。表示している版を渡す。 */
    fun save(role: AgentRole, settings: RoleSettings?) {
        if (_state.value.saving) return
        _state.update { it.copy(saving = true, error = null) }
        reads += 1 // 読んでいる最中の答えは、この保存の答えより古い
        scope.launch {
            try {
                val shown = _state.value.view?.revision
                when (val result = commands.setAgentSettings(project, role, settings, shown)) {
                    is AgentSettingsSave.Saved -> _state.update { it.copy(view = result.settings) }
                    is AgentSettingsSave.Conflict -> {
                        reads += 1
                        _state.update { it.copy(view = result.latest, error = CONFLICT_MESSAGE) }
                    }
                }
                ensureLoaded()
            } catch (e: RpcException) {
                _state.update { it.copy(error = describe(e)) }
            } finally {
                _state.update { it.copy(saving = false) }
                gate.resume()
            }
        }
    }

    /** 行の編集の結果を保存する (断られたらその理由を出す)。 */
    fun apply(role: AgentRole, edit: Edit) {
        when (edit) {
            is Edit.Ok -> save(role, edit.settings)
            is Edit.Refused -> fail(edit.message)
        }
    }
}

/** [role] の設定 (有効なもの)。 */
fun io.github.tom039224.yhtye.shared.api.generated.AgentSettings.of(role: AgentRole): RoleSettings = when (role) {
    AgentRole.Orchestrator -> orchestrator
    AgentRole.Implementer -> implementer
    AgentRole.Investigator -> investigator
    AgentRole.Reviewer -> reviewer
    AgentRole.Unrecognized -> orchestrator
}

/** [role] の層の設定 (その層が持っていなければ null = 継承)。 */
fun io.github.tom039224.yhtye.shared.api.generated.AgentSettingsLayer?.of(role: AgentRole): RoleSettings? = when (role) {
    AgentRole.Orchestrator -> this?.orchestrator
    AgentRole.Implementer -> this?.implementer
    AgentRole.Investigator -> this?.investigator
    AgentRole.Reviewer -> this?.reviewer
    AgentRole.Unrecognized -> null
}

// ---- ハーネス -----------------------------------------------------------------------------------------

data class HarnessSettingsState(
    /** 検出の結果 (最初の読み取りが終わるまで null)。 */
    val harnesses: List<HarnessDetection>? = null,
    val rowErrors: Map<String, String> = emptyMap(),
    val error: String? = null,
    val busy: Boolean = false,
    val detecting: Boolean = false,
    /** 入力欄の中身を保存した値に戻す合図 (読み直すたびに増える)。 */
    val epoch: Int = 0,
)

/** 保存した手動パスの入力欄の初期値 (ハーネスごと。無ければ空)。 */
fun draftsOf(list: List<HarnessDetection>): Map<String, String> = list.associate { it.id to (it.overridePath ?: "") }

/**
 * 「ハーネス」の節: 各ハーネスが入っているか、見つかった場所、主実行ファイルの手動パス (自動検出より優先)。
 * 開くとコアが探し直す。他の端末の変更は読み直す (パスを入力中なら「再読込」のバナー)。
 */
class HarnessSettingsController(
    private val commands: SettingsCommands,
    private val scope: CoroutineScope,
    val editing: SettingsEditing = SettingsEditing(),
) {
    private val _state = MutableStateFlow(HarnessSettingsState())
    val state: StateFlow<HarnessSettingsState> = _state.asStateFlow()
    private var reads = 0

    /** 保存していない手動パスが入力欄にある (画面が教える)。 */
    var unsaved: Boolean = false

    private val gate = SettingsChangeGate(
        scope = scope,
        changes = commands.changes,
        kinds = setOf(SettingsKind.Harnesses),
        reload = ::reload,
        isBusy = { _state.value.busy },
        isEditing = { unsaved || editing.isEditing },
    )

    val stale: StateFlow<Boolean> get() = gate.stale

    fun reload() {
        val read = ++reads
        scope.launch {
            try {
                val list = commands.getHarnesses()
                if (read != reads) return@launch
                _state.update { it.copy(harnesses = list, error = null, epoch = it.epoch + 1) }
            } catch (e: RpcException) {
                if (read == reads) _state.update { it.copy(error = describe(e)) }
            }
        }
    }

    fun reloadNow() = gate.reloadNow()

    /** 各ハーネスを探し直す。最初の読み取りが失敗していたときだけ入力欄を埋める (そうでなければ入力中の文字を残す)。 */
    fun detect() {
        _state.update { it.copy(busy = true, detecting = true, error = null, rowErrors = emptyMap()) }
        reads += 1
        scope.launch {
            try {
                val list = commands.detectHarnesses()
                _state.update { it.copy(harnesses = list, epoch = if (it.harnesses == null) it.epoch + 1 else it.epoch) }
            } catch (e: RpcException) {
                _state.update { it.copy(error = describe(e)) }
            } finally {
                _state.update { it.copy(busy = false, detecting = false) }
                gate.resume()
            }
        }
    }

    /** [harness] の手動パスを [path] にする (null: 消して自動検出に戻す)。成功したか。 */
    suspend fun setPath(harness: String, path: String?): Boolean {
        _state.update { it.copy(busy = true, rowErrors = it.rowErrors - harness) }
        reads += 1
        return try {
            val list = commands.setHarnessPath(harness, path)
            _state.update { it.copy(harnesses = list) }
            true
        } catch (e: RpcException) {
            _state.update { it.copy(rowErrors = it.rowErrors + (harness to describe(e))) }
            false
        } finally {
            _state.update { it.copy(busy = false) }
            gate.resume()
        }
    }

    /** 画面が見つけた入力の誤り (パスが空など)。 */
    fun rowError(harness: String, message: String) = _state.update { it.copy(rowErrors = it.rowErrors + (harness to message)) }
}

const val EMPTY_PATH_MESSAGE: String = "パスを入力してください (自動検出にするには「自動検出に戻す」)"

// ---- 秘密の環境変数 -----------------------------------------------------------------------------------

data class SecretEnvState(
    /** 登録されている名前 (読むまで null)。値は返らない。 */
    val names: List<String>? = null,
    val error: String? = null,
    val busy: Boolean = false,
)

/** [SecretEnvController.register] の結果。 */
enum class RegisterResult { Saved, Invalid, Failed }

/**
 * 「秘密の環境変数」の節: エージェントに渡す環境変数 (API キーなど) の名前の一覧。値は PC の OS のキーリングに保存され、
 * 送るのは 1 回だけ (ここにも画面にも残さない)。他の端末の変更は読み直す (入力中なら「再読込」のバナー)。
 */
class SecretEnvController(
    private val commands: SettingsCommands,
    private val scope: CoroutineScope,
    val editing: SettingsEditing = SettingsEditing(),
) {
    private val _state = MutableStateFlow(SecretEnvState())
    val state: StateFlow<SecretEnvState> = _state.asStateFlow()
    private var reads = 0

    private val gate = SettingsChangeGate(
        scope = scope,
        changes = commands.changes,
        kinds = setOf(SettingsKind.Secrets),
        reload = ::reload,
        isBusy = { _state.value.busy },
        isEditing = { editing.isEditing },
    )

    val stale: StateFlow<Boolean> get() = gate.stale

    fun reload() {
        val read = ++reads
        scope.launch {
            try {
                val names = commands.listSecretEnv()
                if (read == reads) _state.update { it.copy(names = names) }
            } catch (e: RpcException) {
                if (read == reads) _state.update { it.copy(error = describe(e)) }
            }
        }
    }

    fun reloadNow() = gate.reloadNow()

    fun clearError() = _state.update { it.copy(error = null) }

    /**
     * [name] を [value] で登録する (同じ名前は上書き)。名前か値が誤りなら [RegisterResult.Invalid] (理由は [SecretEnvState.error])、
     * 失敗したら [RegisterResult.Failed] (画面は値を捨てる: 秘密を画面に残さない)。
     */
    suspend fun register(name: String, value: String): RegisterResult {
        val trimmed = name.trim()
        val problem = secretNameError(trimmed) ?: if (value.isEmpty()) "値を入力してください" else null
        if (problem != null) {
            _state.update { it.copy(error = problem) }
            return RegisterResult.Invalid
        }
        _state.update { it.copy(busy = true, error = null) }
        reads += 1
        return try {
            val names = commands.setSecretEnv(trimmed, value)
            _state.update { it.copy(names = names) }
            RegisterResult.Saved
        } catch (e: RpcException) {
            _state.update { it.copy(error = describe(e)) }
            RegisterResult.Failed
        } finally {
            _state.update { it.copy(busy = false) }
            gate.resume()
        }
    }

    suspend fun remove(name: String) {
        _state.update { it.copy(busy = true, error = null) }
        reads += 1
        try {
            val names = commands.deleteSecretEnv(name)
            _state.update { it.copy(names = names) }
        } catch (e: RpcException) {
            _state.update { it.copy(error = describe(e)) }
        } finally {
            _state.update { it.copy(busy = false) }
            gate.resume()
        }
    }
}
