package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ClientDevice
import io.github.tom039224.yhtye.shared.api.generated.BranchStatuses
import io.github.tom039224.yhtye.shared.api.generated.GitOverview
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.UsageReport
import io.github.tom039224.yhtye.shared.client.ConnectionState

/** [AppStore] が公開するアプリ全体の状態 (不変。`AppStore.state` の値)。PC の `AppState` に当たる。 */
data class AppState(
    val connection: ConnectionState = ConnectionState.Idle,
    /** コアが知っているプロジェクト (`list_projects`。`open` / `running` は動作中の表示に使う)。 */
    val projects: List<ProjectInfo> = emptyList(),
    /** プロジェクトの一覧を一度でも読めたか (読む前の空と、プロジェクトが無い空を分ける)。 */
    val projectsLoaded: Boolean = false,
    /** 表示中のプロジェクト。 */
    val project: ProjectView? = null,
    /** 画面に出す失敗 (新しいものが後。[MAX_ERRORS] 件まで)。 */
    val errors: List<AppError> = emptyList(),
    val busy: Busy = Busy(),
    /** 表示中のプロジェクトの git の概要 (ブランチの木・会話のヘッダ)。 */
    val git: GitView? = null,
    /** 表示中のプロジェクトのブランチの順 (読むまでは null)。 */
    val branchOrder: BranchOrderView? = null,
    val usage: UsageView = UsageView(),
    val host: HostView = HostView(),
    /** ペアリング済みの端末 (コアの `list_clients`。並びはコアのまま)。左パネルの下端でホストの下に出す。 */
    val clients: List<ClientDevice> = emptyList(),
)

/** 応答を待っている操作。 */
data class Busy(val opening: Boolean = false, val sending: Boolean = false, val compacting: Boolean = false)

data class AppError(val id: Long, val message: String)

/** 表示中のプロジェクトの git の概要。 */
data class GitView(
    val project: String,
    val overview: GitOverview? = null,
    /** 最後の読み取りが失敗した理由。 */
    val error: String? = null,
    val loading: Boolean = false,
    /**
     * git を最後に読んだ後に作られたチャット (`chat_created`。他のクライアントが新しい作業ツリーに作ったかも):
     * 読み直すまでは作業ツリーが一覧に無くても「見つからない」にしない ([placeOf] の `awaiting`)。
     */
    val awaiting: List<String> = emptyList(),
    /** ブランチごとの PR / マージの状態 (BRANCHES の木の点)。 */
    val statuses: BranchStatuses? = null,
    /** [statuses] を最後に読んだ時刻 (エポックからのミリ秒)。 */
    val statusesAt: Long = 0,
)

/** プロジェクトのブランチの順 (コアのリスト。新しく使った順): 作業ツリーのパスと、作業ツリーの無いブランチの名前。 */
data class BranchOrderView(val project: String, val keys: List<String>)

/** サブスクリプションの使用量 (コアがハーネスから読む)。 */
data class UsageView(
    val report: UsageReport? = null,
    /** 最後の読み取りが失敗した理由 (そのときは値を持たない)。 */
    val error: String? = null,
    val loading: Boolean = false,
)

/** コアが動いているマシン (ping の最後の応答)。 */
data class HostView(
    /** ホスト名 (最初の応答までは null)。 */
    val name: String? = null,
    /** 最後に ping に答えた時刻 (エポックからのミリ秒)。 */
    val lastPingAt: Long? = null,
    /** PC の Yhtye の版 (`0.1.0`)。ping が版を返さない古い PC では null (互換性の判定は「不明」)。 */
    val version: String? = null,
)

const val MAX_ERRORS: Int = 5
