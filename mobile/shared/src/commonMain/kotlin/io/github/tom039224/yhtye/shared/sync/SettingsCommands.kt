package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentRole
import io.github.tom039224.yhtye.shared.api.generated.AgentSettingsView
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiErrorCode
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.HarnessDetection
import io.github.tom039224.yhtye.shared.api.generated.HarnessModels
import io.github.tom039224.yhtye.shared.api.generated.ModelEfforts
import io.github.tom039224.yhtye.shared.api.generated.RoleSettings
import io.github.tom039224.yhtye.shared.api.generated.SettingsKind
import io.github.tom039224.yhtye.shared.client.ApiException
import io.github.tom039224.yhtye.shared.client.CoreConnection
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow

/**
 * どこかのクライアントが変えた設定 (`settings_changed`)。表示中の設定画面が読み直す。
 * [kind] が null: 何でも変わったかもしれない (イベントを取りこぼした: 再接続か `resync`)。
 */
data class SettingsChange(
    val kind: SettingsKind?,
    val project: String?,
    /** エージェント設定の新しい版 (`kind == AgentSettings`)。 */
    val revision: Long?,
) {
    companion object {
        val EVERYTHING: SettingsChange = SettingsChange(null, null, null)
    }
}

/** [SettingsCommands.setAgentSettings] の結果。 */
sealed interface AgentSettingsSave {
    data class Saved(val settings: AgentSettingsView) : AgentSettingsSave

    /**
     * 読んだ後に他の端末が設定を変えていたので保存しなかった (`conflict`)。[latest] は読み直した今の設定。
     * 画面は「他の端末で設定が変更されていたため、この変更は保存していません」と出して [latest] を表示する。
     */
    data class Conflict(val latest: AgentSettingsView, val message: String) : AgentSettingsSave
}

/**
 * 設定画面 (エージェント・ハーネス・秘密の環境変数) のコマンドと、設定が変わった知らせ。失敗は例外
 * ([io.github.tom039224.yhtye.shared.client.RpcException]) で返し、画面が出す (アプリのエラー一覧には積まない)。
 * PC の `src/store/settingsCommands.ts` に当たる。リモート接続の設定は PC 専用なので無い。
 */
class SettingsCommands internal constructor(private val core: CoreConnection) {
    private val _changes = MutableSharedFlow<SettingsChange>(
        extraBufferCapacity = CHANGE_BUFFER,
        onBufferOverflow = BufferOverflow.DROP_OLDEST,
    )

    /** 設定が (どこかの端末で) 変わった、または変わったかもしれない。 */
    val changes: SharedFlow<SettingsChange> = _changes.asSharedFlow()

    internal fun notify(change: SettingsChange) {
        _changes.tryEmit(change)
    }

    /** ハーネス × モデルの設定。全体 ([project] が null) かプロジェクトの層。 */
    suspend fun getAgentSettings(project: String?): AgentSettingsView =
        core.expect<ApiResponse.AgentSettings>(ApiCommand.GetAgentSettings(project)).settings

    /**
     * 全体かプロジェクトの層の 1 つの役割を置き換える ([settings] が null なら外す)。[expectedRevision] (表示している版)
     * を渡すと、その後に他の端末が変えていたら保存せず [AgentSettingsSave.Conflict] (今の設定を読み直して返す)。
     */
    suspend fun setAgentSettings(
        project: String?,
        role: AgentRole,
        settings: RoleSettings?,
        expectedRevision: Long?,
    ): AgentSettingsSave {
        val command = ApiCommand.SetAgentSettings(project, role, settings, expectedRevision)
        return try {
            AgentSettingsSave.Saved(core.expect<ApiResponse.AgentSettings>(command).settings)
        } catch (e: ApiException) {
            if (e.error.code != ApiErrorCode.Conflict) throw e
            AgentSettingsSave.Conflict(getAgentSettings(project), e.error.message)
        }
    }

    /** ハーネスが出しているモデル (コアがハーネスから読む。コアでキャッシュされる)。 */
    suspend fun listHarnessModels(harness: String, refresh: Boolean = false): HarnessModels =
        core.expect<ApiResponse.HarnessModels>(ApiCommand.ListHarnessModels(harness, refresh)).models

    /** モデル一覧に effort が無かったモデルの effort (Stage 7d)。 */
    suspend fun listModelEfforts(harness: String, model: String): ModelEfforts =
        core.expect<ApiResponse.ModelEfforts>(ApiCommand.ListModelEfforts(harness, model)).efforts

    /** 各ハーネスが入っているか、実行ファイルの場所。 */
    suspend fun getHarnesses(): List<HarnessDetection> = core.expect<ApiResponse.Harnesses>(ApiCommand.GetHarnesses).harnesses

    /** [getHarnesses] と同じで、各ハーネスから読んだモデルも忘れる (探し直す)。 */
    suspend fun detectHarnesses(): List<HarnessDetection> =
        core.expect<ApiResponse.Harnesses>(ApiCommand.DetectHarnesses).harnesses

    /** ハーネスの実行ファイルのパス (null: 自動で探す)。 */
    suspend fun setHarnessPath(harness: String, path: String?): List<HarnessDetection> =
        core.expect<ApiResponse.Harnesses>(ApiCommand.SetHarnessPath(harness, path)).harnesses

    /** 登録された秘密の環境変数の名前 (値は返らない)。 */
    suspend fun listSecretEnv(): List<String> = core.expect<ApiResponse.SecretEnv>(ApiCommand.ListSecretEnv).names

    /** [value] を PC の OS のキーリングに [name] で保存する (前の値は置き換え)。値はここに残さない。 */
    suspend fun setSecretEnv(name: String, value: String): List<String> =
        core.expect<ApiResponse.SecretEnv>(ApiCommand.SetSecretEnv(name, value)).names

    suspend fun deleteSecretEnv(name: String): List<String> =
        core.expect<ApiResponse.SecretEnv>(ApiCommand.DeleteSecretEnv(name)).names

    private companion object {
        const val CHANGE_BUFFER = 64
    }
}
