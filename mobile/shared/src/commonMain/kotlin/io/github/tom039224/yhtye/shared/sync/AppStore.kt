package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.LoggedEvent
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.CoreConnection
import io.github.tom039224.yhtye.shared.client.RpcException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.coroutines.ContinuationInterceptor
import kotlin.coroutines.CoroutineContext
import kotlin.time.Clock
import kotlin.time.ExperimentalTime

/**
 * コアの状態を写し取って最新に保つ同期層 (PC の `src/store/app.ts` の `AppStore` の移植)。アプリのプロセスに
 * 1 つ置き、画面 (アクティビティ) が無くても動き続けられる。状態は [state] (不変のデータ)。
 *
 * - 読み込み: イベントを溜めながら `get_snapshot` → スナップショットの直近 [Options.historyWindow] 件から
 *   `list_events` をページで読む (会話の履歴) → 溜めたイベントを流す。
 * - 定常: durable イベントは `cursor + 1` で届くはず。古いものは重複として捨て、飛び (と、まだ見ていない
 *   durable の後の live) は `cursor` からの `list_events` で埋める。
 * - 再接続 ([ConnectionState.Connected.session] が変わった): プロジェクトを開き直し (冪等。コアが再起動したかも)、
 *   `cursor` から追いつく。切れた接続のストリーミング途中の本文は捨てる。close 1013 (コアの送信が追いつかない)
 *   も再接続 ([io.github.tom039224.yhtye.shared.client.YhtyeClient] がする) なので同じ。
 * - 他のクライアント (T-65): アプリ全体のイベント (`project` が空) はプロジェクト一覧・設定が変わった、
 *   またはこのクライアント向けのイベントが捨てられた (`resync`: 再接続と同じく追いつく) という知らせ。
 * - クライアント一覧 (T-74、PC は T-73): 左パネルの下端の端末。つながるたび (再接続を含む) と `resync` で `list_clients` を読み、
 *   `clients_changed` で置き換える。
 *
 * 操作は PC と同じく楽観的に状態を変えず、イベントで変わるのを待つ (コアが返したチャットの一覧への追加と、
 * 削除・改名の応答の反映だけは PC と同じくすぐ行う)。依存する要求は応答を待ってから次を送る
 * (同じ接続でもコアは要求を並行に実行する、core-design §10)。
 *
 * 状態の変更は 1 本ずつ (コルーチンの中断点の間で) 行う: [scope] のディスパッチャを並列度 1 に絞って使うので、
 * どのスレッドから呼んでもよい。
 */
class AppStore(
    private val core: CoreConnection,
    scope: CoroutineScope,
    /** この端末の設定 (最後のプロジェクト・プロジェクトごとのチャット)。 */
    private val prefs: ClientPrefs = MemoryPrefs(),
    /** コンポーザの下書き (別のチャット・プロジェクトを出している間も残る)。 */
    val drafts: Drafts = PrefsDrafts(prefs),
    private val options: Options = Options(),
    private val now: () -> Long = ::epochMillis,
) {
    data class Options(
        /** 1 回の `list_events` で読む件数。 */
        val eventPage: Long = 500,
        /** プロジェクトを開いたときに会話に読むイベントの数 (それより古いものは求めに応じて)。 */
        val historyWindow: Long = 400,
        /** 「さらに前の履歴」1 回の件数。 */
        val historyPage: Long = 400,
        /** git の概要のコミット数。 */
        val gitCommits: Long = 120,
        /** ドメインの変化の後に git を読み直すまでの間 (変化はまとまって来る)。 */
        val gitRefreshMs: Long = 400,
        /** ブランチの PR / マージの状態を使い回す間 (`gh` はネットワークを使う)。 */
        val branchStatusTtlMs: Long = 60_000,
        /** 表示していないプロジェクトで作業が始まった・終わった後にプロジェクト一覧を読み直すまでの間。 */
        val activityRefreshMs: Long = 200,
        /** 使用量を読み直す間隔 (つながっていて前面の間。コアは 1 分キャッシュする)。 */
        val usageRefreshMs: Long = 5 * 60_000,
        /** コアに ping する間隔 (つながっていて前面の間。ホストの表示)。 */
        val pingIntervalMs: Long = 3_000,
        /** ping の応答をこれ以上待たない。 */
        val pingTimeoutMs: Long = 5_000,
        /** プロジェクトを開く・ブランチを作る・マージなど時間の掛かる要求の待ち時間。 */
        val longCallTimeoutMs: Long = 180_000,
        /** 添付の base64 の変換など、大きなデータの処理の実行先。テストは呼び出し元のまま ([EmptyCoroutineContext])。 */
        val workContext: CoroutineContext = Dispatchers.Default,
    )

    private val confined: CoroutineDispatcher =
        (scope.coroutineContext[ContinuationInterceptor] as? CoroutineDispatcher ?: Dispatchers.Default).limitedParallelism(1)
    private val scope = CoroutineScope(scope.coroutineContext + SupervisorJob(scope.coroutineContext[Job]) + confined)

    private val _state = MutableStateFlow(AppState(connection = core.state.value))

    /** アプリ全体の状態。 */
    val state: StateFlow<AppState> = _state.asStateFlow()

    /** 設定画面のコマンドと、設定が変わった知らせ。 */
    val settings: SettingsCommands = SettingsCommands(core)

    /** 会話の添付の画像のメモリキャッシュ ([TranscriptItem.User.attachments] の中身を取る)。 */
    val attachmentPreviews: AttachmentPreviews =
        AttachmentPreviews(core, this.scope, AttachmentPreviews.Options(workContext = options.workContext))

    /** チャットごとの入力中の添付 (ステージ・破棄と状態)。送るのは [sendMessage]。 */
    val attachments: AttachmentComposer =
        AttachmentComposer(
            core,
            this.scope,
            attachmentPreviews,
            AttachmentComposer.Options(stageTimeoutMs = options.longCallTimeoutMs, workContext = options.workContext),
        )

    private val _usersTurns = MutableSharedFlow<UsersTurn>(
        extraBufferCapacity = USERS_TURN_BUFFER,
        onBufferOverflow = BufferOverflow.DROP_OLDEST,
    )

    /** オーケストレータの手が止まりユーザーの番になった (表示していないプロジェクトのものも)。通知の元。 */
    val usersTurns: SharedFlow<UsersTurn> = _usersTurns.asSharedFlow()

    // ここから下は [confined] でだけ触る
    private var buffer = mutableListOf<ApiEvent>()
    private var syncing = false
    private var session: Long? = null
    private var loadToken = 0L
    private var nextErrorId = 1L
    private var gitTimer: Job? = null
    private var gitToken = 0L
    private var activityTimer: Job? = null
    private var activityToken = 0L
    private var usageTimer: Job? = null
    private var pingTimer: Job? = null
    private var pinging = false
    private var clientsToken = 0L
    /** 読み・ドラッグ・`branch_order_changed` ごとに進める: 最後のものだけを残す。 */
    private var branchOrderToken = 0L
    private var foreground = true
    private var started = false
    private val background = BackgroundTurns()

    /** コアのイベントと接続状態を受け始める (1 回だけ)。 */
    fun start() {
        scope.launch {
            if (started) return@launch
            started = true
            launch { core.events.collect(::onEvent) }
            launch { core.state.collect(::onConnection) }
        }
    }

    /** 止める (以後は何もしない)。 */
    fun close() {
        scope.cancel()
    }

    /**
     * アプリが前面に来た・裏に回った。前面に来たら git を読み直す (PC のウィンドウのフォーカスと同じ: 外での改名が
     * 出るように)。使用量と ping は前面にある間だけ読む (裏では電池を使わない)。
     */
    fun setForeground(value: Boolean) {
        scope.launch {
            if (foreground == value) return@launch
            foreground = value
            val connected = _state.value.connection is ConnectionState.Connected
            if (!value) {
                stopPolling()
            } else if (connected) {
                startPolling()
                launch { refreshGit() }
            }
        }
    }

    // ---- 操作 -----------------------------------------------------------------------------------

    suspend fun refreshProjects() {
        onConfined {
            val r = report<ApiResponse.Projects>(ApiCommand.ListProjects) ?: return@onConfined
            set { it.copy(projects = r.projects, projectsLoaded = true) }
        }
    }

    /**
     * プロジェクトをユーザーが並べた順にする ([ids] は一覧の全部)。すぐ出し、コアの一覧に置き換える。断られたら
     * コアの順に戻す。
     */
    suspend fun reorderProjects(ids: List<String>) {
        onConfined {
            val before = _state.value.projects
            set { it.copy(projects = ids.flatMap { id -> before.filter { p -> p.id == id } }) }
            val r = report<ApiResponse.Projects>(ApiCommand.ReorderProjects(ids))
            if (r != null) set { it.copy(projects = r.projects) } else refreshProjects()
        }
    }

    /**
     * 表示中のプロジェクトのブランチをユーザーが並べた順にする: [keys] は画面に出ているものの順 (作業ツリーのパスと
     * ブランチ名)。ほかのキーはコアがそのまま続ける。すぐ出す。
     */
    suspend fun reorderBranches(keys: List<String>) {
        onConfined {
            val project = _state.value.project?.info?.id ?: return@onConfined
            val token = ++branchOrderToken
            val known = _state.value.branchOrder?.takeIf { it.project == project }?.keys.orEmpty()
            set { it.copy(branchOrder = BranchOrderView(project, mergeOrder(keys, known))) }
            val r = report<ApiResponse.BranchOrder>(ApiCommand.ReorderBranches(project, keys))
            if (r == null) refreshBranchOrder() else if (token == branchOrderToken) setBranchOrder(project, r.keys)
        }
    }

    /** 表示中のプロジェクトのブランチの順を読み直す (失敗したら前のまま: 順は好みでしかない)。 */
    suspend fun refreshBranchOrder() {
        onConfined {
            val project = _state.value.project?.info?.id ?: return@onConfined
            val token = ++branchOrderToken
            val r = try {
                core.expect<ApiResponse.BranchOrder>(ApiCommand.GetBranchOrder(project))
            } catch (e: RpcException) {
                return@onConfined
            }
            if (token == branchOrderToken) setBranchOrder(project, r.keys)
        }
    }

    private fun setBranchOrder(project: String, keys: List<String>) {
        if (_state.value.project?.info?.id != project) return
        set { it.copy(branchOrder = BranchOrderView(project, keys)) }
    }

    suspend fun openProject(path: String) {
        onConfined {
            set { it.copy(busy = it.busy.copy(opening = true)) }
            try {
                val r = report<ApiResponse.Project>(ApiCommand.OpenProject(path), options.longCallTimeoutMs) ?: return@onConfined
                // プロジェクトを開くのは使うこと: 先頭になる (コアもそうする)
                set { it.copy(projects = toFrontProject(it.projects, r.project)) }
                load(r.project)
            } finally {
                set { it.copy(busy = it.busy.copy(opening = false)) }
            }
        }
    }

    /**
     * 選択中のチャットに送る。入力中の添付 ([attachments]) のステージ済みのものも付ける (本文が空でも添付があれば送れる。
     * [canSendMessage])。受け付けられたか (受け付けられなければコンポーザは文と添付を残す)。アップロード中・失敗した添付が
     * あるときは送らずエラーにする。
     */
    suspend fun sendMessage(text: String): Boolean = onConfined {
        val view = _state.value.project
        val chat = view?.selectedChat ?: return@onConfined false
        val project = view.info.id
        val pending = attachments.pending(project, chat)
        if (!pending.isSettled()) {
            pushError("send_user_message failed: wait for the attachments to be uploaded (or remove the ones that failed)")
            return@onConfined false
        }
        val ids = pending.stagedIds()
        set { it.copy(busy = it.busy.copy(sending = true)) }
        try {
            val accepted = report<ApiResponse.Accepted>(ApiCommand.SendUserMessage(project, chat, text, ids)) != null
            if (accepted) attachments.markSent(project, chat, ids)
            accepted
        } finally {
            set { it.copy(busy = it.busy.copy(sending = false)) }
        }
    }

    /**
     * 通知のタップ: [project] の [chat] を出す (別のプロジェクトなら開く)。アプリを通知から起動したときは、つながって一覧が
     * 読めるまで待つ (最後のプロジェクトを開き直す途中も待つ)。プロジェクトかチャットが無ければ false。
     */
    suspend fun showChat(project: String, chat: String): Boolean {
        state.first { it.connection is ConnectionState.Connected && it.projectsLoaded }
        state.first { !it.busy.opening }
        return onConfined {
            if (_state.value.project?.info?.id != project) {
                val info = _state.value.projects.find { it.id == project } ?: return@onConfined false
                openProject(info.path)
            }
            val view = _state.value.project
            if (view?.info?.id != project || view.phase != LoadPhase.Ready || view.chats.none { it.id == chat }) return@onConfined false
            selectChatNow(chat)
            true
        }
    }

    /** 選択中のチャットのオーケストレータのターンを止める。 */
    suspend fun cancelTurn() {
        onConfined {
            val view = _state.value.project
            val chat = view?.selectedChat ?: return@onConfined
            report<ApiResponse.Accepted>(ApiCommand.CancelOrchestratorTurn(view.info.id, chat))
        }
    }

    /** 選択中のチャットのアイドルなオーケストレータに文脈を圧縮させる (`/compact`)。断られたらエラーを出す。 */
    suspend fun compactChat() {
        onConfined {
            val view = _state.value.project
            val chat = view?.selectedChat ?: return@onConfined
            if (_state.value.busy.compacting) return@onConfined
            set { it.copy(busy = it.busy.copy(compacting = true)) }
            try {
                report<ApiResponse.Accepted>(ApiCommand.CompactChat(view.info.id, chat))
            } finally {
                set { it.copy(busy = it.busy.copy(compacting = false)) }
            }
        }
    }

    /**
     * 別のチャットを出す (プロジェクトごとに覚える)。チャットを開くと、再開するものがあればオーケストレータが
     * 戻ってくる (Yhtye の再起動では何も起動しない) ので `open_chat` をコアに送る。
     */
    fun selectChat(chat: String) {
        scope.launch { selectChatNow(chat) }
    }

    private fun selectChatNow(chat: String) {
        val view = _state.value.project ?: return
        if (view.selectedChat == chat || view.chats.none { it.id == chat }) return
        prefs.set(chatPrefKey(view.info.id), chat)
        updateProject { it.copy(selectedChat = chat, unread = it.unread - chat) }
        scope.launch { openChat(view.info.id, chat) }
    }

    /**
     * 作業ツリー (か、ブランチの作業ツリー。どこにもチェックアウトされていなければ Yhtye が作る) に新しいチャット
     * (オーケストレータはまだ無い) を作り、それを選ぶ。失敗はエラーに出し false。
     */
    suspend fun createChat(worktree: String? = null, branch: String? = null): Boolean = onConfined {
        val project = _state.value.project?.info?.id ?: return@onConfined false
        val r = report<ApiResponse.Chat>(ApiCommand.CreateChat(project, branch, worktree), options.longCallTimeoutMs)
            ?: return@onConfined false
        // ブランチには新しい作業ツリーができたかも: チャットを出す前に一覧に載せる
        if (branch != null && gitReadNeededFor(r.chat.id, r.chat.worktree)) refreshGit()
        adoptChat(project, r.chat)
        true
    }

    /**
     * Yhtye の作業ツリーに新しいブランチを作り、最初のチャット (選ぶ) を作る。[from] の既定はメインのクローンの
     * HEAD。失敗はコアのエラーを投げる (入力欄の横に出す)。
     */
    suspend fun createBranch(name: String, from: String? = null) {
        onConfined {
            val project = _state.value.project?.info?.id ?: return@onConfined
            val r = core.expect<ApiResponse.Chat>(ApiCommand.CreateBranch(project, name, from), options.longCallTimeoutMs)
            // git が先: 概要が作業ツリーを出す前に表示したチャットは「見つからない」に見える
            if (gitReadNeededFor(r.chat.id, r.chat.worktree)) refreshGit()
            adoptChat(project, r.chat)
        }
    }

    /** コアがいま作ったチャット (イベントはまだかも) を一覧に載せて選ぶ。 */
    private fun adoptChat(project: String, chat: ChatInfo) {
        if (_state.value.project?.info?.id != project) return
        updateProject { it.copy(chats = upsertChat(it.chats, chat)) }
        selectChatNow(chat.id)
    }

    /** ユーザーが付けるチャットの題。失敗 (空・長すぎる・知らないチャット) はコアのエラーを投げる。 */
    suspend fun renameChat(chat: String, title: String) {
        onConfined {
            val project = _state.value.project?.info?.id ?: return@onConfined
            val r = core.expect<ApiResponse.Chat>(ApiCommand.RenameChat(project, chat, title))
            if (_state.value.project?.info?.id != project) return@onConfined
            // コアの `chat_titled` が続く。応答をすぐ出すと古い題が一瞬見えない
            updateProject { v -> v.copy(chats = v.chats.map { if (it.id == chat) it.copy(title = r.chat.title) else it }) }
        }
    }

    /**
     * チャットを削除する (作業ツリーは残る)。断られたら (オーケストレータが動いている・グループが終わっていない)
     * コアのエラーを投げる。表示中だったら別のチャットを出す。
     */
    suspend fun deleteChat(chat: String) {
        onConfined {
            val project = _state.value.project?.info?.id ?: return@onConfined
            core.expect<ApiResponse.Accepted>(ApiCommand.DeleteChat(project, chat))
            forgetComposer(project, chat)
            if (_state.value.project?.info?.id != project) return@onConfined
            updateChats { removeChatNow(it, chat) }
        }
    }

    /**
     * チャットを眠らせる: オーケストレータのプロセスを止める (保存したセッションは残るので、次の文で戻ってくる)。
     * 断られたら (ターンが動いている) エラーに出す。
     */
    suspend fun sleepChat(chat: String) {
        onConfined {
            val project = _state.value.project?.info?.id ?: return@onConfined
            report<ApiResponse.Accepted>(ApiCommand.SleepChat(project, chat))
        }
    }

    /**
     * プロジェクトのチャットをみな眠らせる (表示していないプロジェクトも): すべてのオーケストレータのプロセスを止めて、
     * プロジェクトは動いていない扱いになる。一括で、断られたら (どれかのターンが動いている) 何も止めずエラーに出す。
     * 表示していないプロジェクトの一覧は、コアの `session_stopped` を受けて更新する。
     */
    suspend fun sleepProject(project: String) {
        onConfined {
            report<ApiResponse.Accepted>(ApiCommand.SleepProject(project))
        }
    }

    /** タスクを中止する (そのエージェントを止める)。 */
    suspend fun cancelTask(task: String) {
        onConfined {
            val project = _state.value.project?.info?.id ?: return@onConfined
            report<ApiResponse.Accepted>(ApiCommand.CancelTask(project, task))
        }
    }

    suspend fun cancelGroup(group: String) {
        onConfined {
            val project = _state.value.project?.info?.id ?: return@onConfined
            report<ApiResponse.Accepted>(ApiCommand.CancelGroup(project, group))
        }
    }

    /** `merge_blocked` のグループのマージをやり直す。 */
    suspend fun retryGroupMerge(group: String) {
        onConfined {
            val project = _state.value.project?.info?.id ?: return@onConfined
            report<ApiResponse.Accepted>(ApiCommand.RetryGroupMerge(project, group), options.longCallTimeoutMs)
        }
    }

    /**
     * 表示している前の履歴を 1 ページ読む (遅延読み込み。PC の OlderHistory)。失敗は [ProjectView.olderError] に
     * 残す (もう一度呼ぶと再試行)。
     */
    suspend fun loadOlderHistory() {
        onConfined {
            val view = _state.value.project ?: return@onConfined
            if (view.phase != LoadPhase.Ready || view.loadingOlder || view.historyStart <= 1) return@onConfined
            val token = loadToken
            val from = maxOf(1L, view.historyStart - options.historyPage)
            updateProject { it.copy(loadingOlder = true, olderError = null) }
            try {
                val events = mutableListOf<ApiEvent>()
                var after = from - 1
                while (after < view.historyStart - 1) {
                    val cmd = ApiCommand.ListEvents(view.info.id, after, view.historyStart - 1 - after)
                    val page = core.expect<ApiResponse.Events>(cmd)
                    if (token != loadToken) return@onConfined
                    if (page.events.isEmpty()) break
                    events += page.events.map(::toApiEvent)
                    after = page.events.last().seq
                }
                updateProject { prependHistory(it, events, from).copy(loadingOlder = false) }
            } catch (e: RpcException) {
                if (token != loadToken) return@onConfined
                updateProject { it.copy(loadingOlder = false, olderError = describe(e)) }
            }
        }
    }

    /** 表示中のプロジェクトのブランチとコミットのグラフを読み直す。 */
    suspend fun refreshGit() {
        onConfined {
            val project = _state.value.project?.info?.id ?: return@onConfined
            val token = ++gitToken
            val prev = _state.value.git?.takeIf { it.project == project }
            val statuses = prev?.statuses
            val statusesAt = prev?.statusesAt ?: 0
            set { it.copy(git = GitView(project, prev?.overview, error = null, loading = true, awaiting = prev?.awaiting.orEmpty(), statuses = statuses, statusesAt = statusesAt)) }
            try {
                val r = core.expect<ApiResponse.GitOverview>(ApiCommand.GetGitOverview(project, options.gitCommits))
                if (token != gitToken) return@onConfined
                // 待っていたチャットは全部作られた後に読んだ: その作業ツリーは載っている
                set { it.copy(git = GitView(project, r.git, statuses = statuses, statusesAt = statusesAt)) }
                refreshBranchStatuses()
            } catch (e: RpcException) {
                if (token != gitToken) return@onConfined
                set {
                    val git = it.git?.takeIf { g -> g.project == project } ?: GitView(project, awaiting = prev?.awaiting.orEmpty())
                    it.copy(git = git.copy(error = describe(e), loading = false))
                }
            }
        }
    }

    /**
     * ブランチの PR / マージの状態を読み直す (BRANCHES の木の点)。最近読んだものは使い回す (`gh` はネット
     * ワークを使う)。失敗したら前のまま (点は次の読み直しまで「不明」)。
     */
    suspend fun refreshBranchStatuses(force: Boolean = false) {
        onConfined {
            val project = _state.value.project?.info?.id ?: return@onConfined
            val prev = _state.value.git?.takeIf { it.project == project }
            if (!force && prev?.statuses != null && now() - prev.statusesAt < options.branchStatusTtlMs) return@onConfined
            try {
                val r = core.expect<ApiResponse.BranchStatuses>(ApiCommand.GetBranchStatuses(project))
                if (_state.value.project?.info?.id != project) return@onConfined
                val git = _state.value.git?.takeIf { it.project == project }
                    ?: GitView(project, awaiting = prev?.awaiting.orEmpty())
                set { it.copy(git = git.copy(project = project, statuses = r.statuses, statusesAt = now())) }
            } catch (_: RpcException) {
                // 前のまま (上を見よ)。
            }
        }
    }

    /**
     * 使用量を読み直す。[refresh] はコアにハーネスへ問い合わせ直させる (メーターのタップ)。失敗したら値を持たない
     * (メーターは「—」と理由)。
     */
    suspend fun refreshUsage(refresh: Boolean = false) {
        onConfined {
            if (_state.value.usage.loading) return@onConfined
            set { it.copy(usage = it.usage.copy(loading = true)) }
            val usage = try {
                UsageView(report = core.expect<ApiResponse.Usage>(ApiCommand.GetUsage(refresh)).usage)
            } catch (e: RpcException) {
                UsageView(error = describe(e))
            }
            set { it.copy(usage = usage) }
        }
    }

    fun dismissError(id: Long) {
        scope.launch { set { s -> s.copy(errors = s.errors.filter { it.id != id }) } }
    }

    // ---- 同期 ---------------------------------------------------------------------------------

    private suspend fun load(info: ProjectInfo) {
        val token = ++loadToken
        buffer = mutableListOf()
        syncing = true
        val current = _state.value
        val keepGit = current.git?.takeIf { it.project == info.id }
        // 離れるプロジェクトは以後は一覧の値で判断する: ビューが言っていたことを書いておく
        val left = current.project?.takeIf { it.info.id != info.id }
        set { it.copy(project = newProjectView(info), projects = rememberRunning(it.projects, left), git = keepGit) }
        if (left != null) scheduleActivityRefresh()
        background.forget(info.id)
        prefs.set(LAST_PROJECT_KEY, info.path)
        scope.launch { refreshGit() }
        scope.launch { refreshBranchOrder() }
        try {
            val snap = core.expect<ApiResponse.Snapshot>(ApiCommand.GetSnapshot(info.id)).snapshot
            if (token != loadToken) return
            val remembered = prefs.get(chatPrefKey(info.id))
            updateProject { applySnapshot(it, snap, options.historyWindow, remembered) }
            catchUp(token)
            if (token != loadToken) return
            updateProject { it.copy(phase = LoadPhase.Ready) }
            openSelectedChat()
        } catch (e: RpcException) {
            if (token != loadToken) return
            val message = "could not load ${info.name}: ${describe(e)}"
            syncing = false
            updateProject { it.copy(phase = LoadPhase.Error, loadError = message) }
            pushError(message)
        }
    }

    /** `cursor` より後の保存済みイベントを全部適用し、溜めたイベントを流す。 */
    private suspend fun catchUp(token: Long) {
        syncing = true
        try {
            while (true) {
                val project = _state.value.project ?: return
                if (token != loadToken) return
                val cmd = ApiCommand.ListEvents(project.info.id, project.cursor, options.eventPage)
                val page = core.expect<ApiResponse.Events>(cmd)
                if (token != loadToken) return
                updateChats { v -> foldDurable(v, page.events.map(::toApiEvent)) }
                if (!page.more) break
            }
        } finally {
            if (token == loadToken) syncing = false
        }
        val buffered = buffer
        buffer = mutableListOf()
        buffered.forEach(::onEvent)
    }

    private fun requestCatchUp() {
        if (syncing) return
        val token = loadToken
        scope.launch {
            try {
                catchUp(token)
            } catch (e: RpcException) {
                pushError("catching up with the event log failed: ${describe(e)}")
            }
        }
    }

    /** 続きの durable イベントを順に適用する (`cursor + 1` でないものは飛ばす)。ユーザーの番も見る。 */
    private fun foldDurable(view: ProjectView, events: List<ApiEvent>): ProjectView = events.fold(view) { v, ev ->
        if (ev.seq != v.cursor + 1) return@fold v
        val next = applyDurable(v, ev)
        usersTurnAfter(v, next, ev)?.let { _usersTurns.tryEmit(it) }
        next
    }

    private fun onEvent(ev: ApiEvent) {
        if (isAppWide(ev)) {
            onAppWide(ev.body)
            return
        }
        val view = _state.value.project
        if (view == null || ev.project != view.info.id) {
            // 表示していない: 出すのは作業が動いているか (リング) とユーザーの番だけ
            if (changesActivity(ev)) scheduleActivityRefresh()
            background.observe(ev)?.let { chat -> scope.launch { checkBackgroundTurn(ev, chat) } }
            return
        }
        if (view.phase == LoadPhase.Error) return
        val created = ((ev.body as? ApiEventBody.Domain)?.event as? DomainEvent.ChatCreated)?.chat
        if (!ev.live && created != null) {
            // 作業ツリーは新しいかも: git を読み直す間は「見つからない」にしない。すぐ読む
            if (gitReadNeededFor(created.id, created.worktree)) refreshGitNow()
        } else if ((!ev.live && changesGit(ev)) || endsOrchestratorTurn(ev)) {
            scheduleGitRefresh()
        }
        if (syncing) {
            buffer += ev
            return
        }
        if (ev.live) {
            // 適用済みより古い: そのブロックは (追いつきの後に溜めたものから) もう完成しているかも。出し直すと本文が重なる
            if (ev.seq < view.cursor) return
            if (ev.seq > view.cursor) requestCatchUp()
            updateProject { applyLive(it, ev) }
            return
        }
        if (ev.seq <= view.cursor) return // 適用済み (ログか溜めたものから)
        if (ev.seq == view.cursor + 1) {
            updateChats { foldDurable(it, listOf(ev)) }
            return
        }
        buffer += ev
        requestCatchUp()
    }

    private fun onConnection(next: ConnectionState) {
        val wasConnected = _state.value.connection is ConnectionState.Connected
        set { it.copy(connection = next) }
        if (next !is ConnectionState.Connected) {
            if (wasConnected) updateProject(::clearStreaming)
            stopPolling()
            return
        }
        val previous = session
        if (previous == next.session) return
        session = next.session
        if (foreground) startPolling()
        scope.launch { refreshClients() }
        if (previous == null) {
            scope.launch { reopenLast() }
            return
        }
        // つながり直した (間の状態を通らずに番号が変わっても): 切れていた間のものを取り直す
        updateProject(::clearStreaming)
        settings.notify(SettingsChange.EVERYTHING)
        val view = _state.value.project
        if (view == null) {
            scope.launch { reopenLast() }
            return
        }
        scope.launch { refreshProjects() }
        scope.launch { resync(view.info) }
        scope.launch { refreshGit() }
        scope.launch { refreshBranchOrder() }
    }

    /** アプリ全体のイベント (プロジェクトが無い): 他のクライアントから、または追いつきのヒント。 */
    private fun onAppWide(body: ApiEventBody) {
        when (body) {
            is ApiEventBody.ProjectsChanged -> scope.launch { refreshProjects() }
            is ApiEventBody.BranchOrderChanged -> {
                branchOrderToken++ // 読んでいる途中の答えより新しい
                setBranchOrder(body.project, body.keys)
            }
            is ApiEventBody.SettingsChanged -> settings.notify(SettingsChange(body.kind, body.project, body.revision))
            ApiEventBody.Resync -> catchUpAll()
            is ApiEventBody.ClientsChanged -> {
                clientsToken++ // 読んでいる途中の答えより新しい
                set { it.copy(clients = body.clients.devices) }
            }
            else -> Unit // リモート接続のイベントは PC 専用 (届かない)
        }
    }

    /**
     * このクライアント向けのイベントが捨てられた (追いつけなかった): それで変わったはずのものを読み直す —
     * プロジェクト一覧、クライアント一覧、表示中のプロジェクトのログと git、表示中のチャット (起動中か)、表示中の設定。
     */
    private fun catchUpAll() {
        scope.launch { refreshProjects() }
        scope.launch { refreshClients() }
        settings.notify(SettingsChange.EVERYTHING)
        if (_state.value.project?.phase != LoadPhase.Ready) return
        requestCatchUp()
        scope.launch { refreshGit() }
        scope.launch { refreshBranchOrder() }
        openSelectedChat()
    }

    /** 最初につながったとき: 最後に開いていたプロジェクトを開く。 */
    private suspend fun reopenLast() {
        refreshProjects()
        val last = prefs.get(LAST_PROJECT_KEY) ?: return
        val current = _state.value
        if (current.project != null || current.busy.opening) return
        if (current.projects.any { it.path == last }) openProject(last)
    }

    /** 再接続の後: プロジェクトが開いていることを確かめてから追いつく。 */
    private suspend fun resync(info: ProjectInfo) {
        if (_state.value.project?.phase != LoadPhase.Ready) {
            openProject(info.path)
            return
        }
        val r = report<ApiResponse.Project>(ApiCommand.OpenProject(info.path), options.longCallTimeoutMs) ?: return
        if (_state.value.project?.info?.id != info.id) return
        updateProject { it.copy(info = r.project) }
        requestCatchUp()
        openSelectedChat() // コアが再起動したかも
    }

    /**
     * コアに、ユーザーが [chat] を見ていると知らせる: 保存されたセッションか再開する作業があればオーケストレータを
     * 起動し、起動中かを答える (セッションが上がるまで出す)。失敗はエラーに出す。
     */
    private suspend fun openChat(project: String, chat: String) {
        val r = report<ApiResponse.ChatOpened>(ApiCommand.OpenChat(project, chat))
        if (_state.value.project?.info?.id != project) return
        updateProject { markResuming(it, chat, r?.starting ?: false) }
    }

    /** 表示中のチャットの [openChat] (プロジェクトの読み込み・再接続の後)。 */
    private fun openSelectedChat() {
        val view = _state.value.project ?: return
        val chat = view.selectedChat ?: return
        scope.launch { openChat(view.info.id, chat) }
    }

    /** 表示していないプロジェクトのターンが正常に終わった: スナップショットでユーザーの番かを確かめる。 */
    private suspend fun checkBackgroundTurn(ev: ApiEvent, chat: String) {
        val snap = try {
            core.expect<ApiResponse.Snapshot>(ApiCommand.GetSnapshot(ev.project)).snapshot
        } catch (e: RpcException) {
            return // 次のターンの終わりでまた確かめる
        }
        if (!isUsersTurn(snap.state, snap.sessions, chat)) return
        val title = snap.chats.find { it.id == chat }?.title
        _usersTurns.tryEmit(UsersTurn(ev.project, chat, title, background.lastMessage(ev.project, chat), ev.tsMs))
    }

    private fun startPolling() {
        if (usageTimer?.isActive != true) {
            usageTimer = scope.launch {
                while (isActive) {
                    launch { refreshUsage() }
                    delay(options.usageRefreshMs)
                }
            }
        }
        if (pingTimer?.isActive != true) {
            pingTimer = scope.launch {
                while (isActive) {
                    launch { ping() }
                    delay(options.pingIntervalMs)
                }
            }
        }
    }

    private fun stopPolling() {
        usageTimer?.cancel()
        usageTimer = null
        pingTimer?.cancel()
        pingTimer = null
    }

    /**
     * ペアリング済みの端末を読む (最後に始めた読みか、その後の `clients_changed` が勝つ)。失敗は出さない: 下端は前の一覧のまま、
     * 次の再接続か `resync` でまた読む (T-73 より前のコアは `list_clients` を知らない)。
     */
    private suspend fun refreshClients() {
        val token = ++clientsToken
        val r = try {
            core.expect<ApiResponse.Clients>(ApiCommand.ListClients)
        } catch (e: RpcException) {
            return
        }
        if (token == clientsToken) set { it.copy(clients = r.clients.devices) }
    }

    /** 1 回の疎通確認。応答が無くてもエラーにはしない (ホストの表示が最後の応答からの時間を数える)。 */
    private suspend fun ping() {
        if (pinging) return
        pinging = true
        try {
            val pong = try {
                withTimeoutOrNull(options.pingTimeoutMs) { core.expect<ApiResponse.Pong>(ApiCommand.Ping, options.pingTimeoutMs) }
            } catch (e: RpcException) {
                null // 次の ping でまた試す
            } ?: return
            set { it.copy(host = HostView(pong.host, now(), pong.version.trim().ifEmpty { null }, pong.headless)) }
        } finally {
            pinging = false
        }
    }

    /** 作業が動いているかを近いうちに読み直す (表示していないプロジェクトのイベントは畳み込まないのでコアに聞く)。 */
    private fun scheduleActivityRefresh() {
        if (activityTimer?.isActive == true) return
        activityTimer = scope.launch {
            delay(options.activityRefreshMs)
            refreshActivity()
        }
    }

    /** 全プロジェクトの `open` / `running` をコアから取る (最後の応答が勝つ)。 */
    private suspend fun refreshActivity() {
        val token = ++activityToken
        val r = report<ApiResponse.Projects>(ApiCommand.ListProjects) ?: return
        if (token != activityToken) return
        set { s ->
            val projects = mergeActivity(s.projects, r.projects)
            if (projects === s.projects) s else s.copy(projects = projects)
        }
    }

    /**
     * [chat] が作られた後に git を読む必要があるか。あれば、読み終えるまで待つチャットとして覚える ([GitView.awaiting])。
     * 要らないのは、作業ツリーがいまの概要に載っているか、そのチャットのために読んでいる最中のとき。この端末の
     * `create_chat` / `create_branch` (チャットを出す前に git を読む) と、追いつきの後に溜めたものから流し直したイベント。
     */
    private fun gitReadNeededFor(chat: String, worktree: String): Boolean {
        val project = _state.value.project?.info?.id ?: return true
        val git = _state.value.git?.takeIf { it.project == project } ?: return true
        if (git.overview?.worktrees?.any { it.path == worktree } == true) return false
        if (git.loading && chat in git.awaiting) return false
        set { it.copy(git = git.copy(awaiting = git.awaiting + chat, error = null, loading = true)) }
        return true
    }

    /** git をすぐ読み直す (待っている遅延の読み直しは捨てる)。 */
    private fun refreshGitNow() {
        gitTimer?.cancel()
        gitTimer = null
        scope.launch { refreshGit() }
    }

    private fun scheduleGitRefresh() {
        if (gitTimer?.isActive == true) return
        gitTimer = scope.launch {
            delay(options.gitRefreshMs)
            refreshGit()
        }
    }

    // ---- 補助 ---------------------------------------------------------------------------------

    /** [block] を状態の変更と同じ並びで実行する。 */
    private suspend fun <T> onConfined(block: suspend () -> T): T = withContext(confined) { block() }

    /** コマンドを実行する。失敗は画面のエラーにして null。 */
    private suspend inline fun <reified T : ApiResponse> report(command: ApiCommand, timeoutMs: Long? = null): T? = try {
        core.expect<T>(command, timeoutMs)
    } catch (e: RpcException) {
        pushError("${commandName(command)} failed: ${describe(e)}")
        null
    }

    private fun pushError(message: String) {
        val error = AppError(nextErrorId++, message)
        set { it.copy(errors = (it.errors + error).takeLast(MAX_ERRORS)) }
    }

    /**
     * チャットを増やす・減らすかもしれない変更 (他のクライアントのものも) 用の [updateProject]: 消えたチャットの下書き・入力中の添付を
     * 消し、表示中のチャットが変わったら (消された、またはチャットの無いプロジェクトに作られた) ユーザーが選んだのと
     * 同じく覚えて開く。
     */
    private fun updateChats(f: (ProjectView) -> ProjectView) {
        val before = _state.value.project
        updateProject(f)
        val after = _state.value.project
        if (before == null || after == null || before.info.id != after.info.id || before.chats == after.chats) return
        val project = after.info.id
        val remaining = after.chats.mapTo(HashSet()) { it.id }
        before.chats.filter { it.id !in remaining }.forEach { forgetComposer(project, it.id) }
        val next = after.selectedChat
        if (next == before.selectedChat) return
        prefs.set(chatPrefKey(project), next)
        if (next != null && after.phase == LoadPhase.Ready) scope.launch { openChat(project, next) }
    }

    /** 消えたチャットのコンポーザが持っていたもの: 下書き・入力中の添付・添付のプレビュー。 */
    private fun forgetComposer(project: String, chat: String) {
        drafts.set(project, chat, "")
        attachments.forgetChat(project, chat)
        attachmentPreviews.forgetChat(project, chat)
    }

    private fun updateProject(f: (ProjectView) -> ProjectView) {
        set { s ->
            val view = s.project ?: return@set s
            val next = f(view)
            if (next === view) s else s.copy(project = next)
        }
    }

    private inline fun set(f: (AppState) -> AppState) {
        _state.value = f(_state.value)
    }

    private companion object {
        const val USERS_TURN_BUFFER = 64
    }
}

/** [p] を先頭に (一覧に無ければ足す、あれば置き換える)。 */
private fun toFrontProject(list: List<ProjectInfo>, p: ProjectInfo): List<ProjectInfo> = listOf(p) + list.filter { it.id != p.id }

/** 保存されたイベント (`list_events`) を流れてくるイベントと同じ形に。 */
internal fun toApiEvent(e: LoggedEvent): ApiEvent = ApiEvent(e.seq, e.tsMs, e.project, e.live, e.body)

@OptIn(ExperimentalTime::class)
private fun epochMillis(): Long = Clock.System.now().toEpochMilliseconds()
