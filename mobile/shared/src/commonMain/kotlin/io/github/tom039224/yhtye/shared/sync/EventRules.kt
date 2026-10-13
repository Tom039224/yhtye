package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.ResultOfStringOrAgentError
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus
import io.github.tom039224.yhtye.shared.api.generated.TaskStatus

// どのイベントが何を変えるか: 同期 ([AppStore]) が使う規則。PC の `src/store/eventRules.ts` と
// `src/store/activity.ts` の移植 (core-design.md §2, §11, §17.12, T-65)。

/** プロジェクトではなくアプリ全体のイベント (コアの `ApiEvent::app_wide`)。 */
fun isAppWide(ev: ApiEvent): Boolean = when (ev.body) {
    is ApiEventBody.ProjectsChanged,
    is ApiEventBody.SettingsChanged,
    ApiEventBody.Resync,
    is ApiEventBody.RemoteStatusChanged,
    is ApiEventBody.RemoteDevicesChanged,
    is ApiEventBody.ClientsChanged,
    -> true
    else -> false
}

/**
 * オーケストレータがターンを終えた: 自分でブランチを改名・切り替えたかもしれない (Stage 8e: ツリーは
 * 作業ツリーが今チェックアウトしているものを出す)。
 */
fun endsOrchestratorTurn(ev: ApiEvent): Boolean {
    val body = ev.body
    return body is ApiEventBody.Agent && body.event is AgentEvent.TurnEnded && isOrchestratorKey(body.session)
}

/** ブランチやコミットが変わったかもしれない durable のドメインイベント。 */
fun changesGit(ev: ApiEvent): Boolean {
    val body = ev.body as? ApiEventBody.Domain ?: return false
    return when (body.event) {
        is DomainEvent.ChatCreated,
        is DomainEvent.GroupCreated,
        is DomainEvent.GroupBaseChanged,
        is DomainEvent.GroupMergeFinished,
        is DomainEvent.GroupCancelled,
        is DomainEvent.WorkspaceReady,
        is DomainEvent.TaskStatusChanged,
        is DomainEvent.TaskCancelled,
        is DomainEvent.StepCompleted,
        -> true
        else -> false
    }
}

/** オーケストレータのターンが正常に (`end_turn`) 終わったなら、そのチャット。中止・エラーは null。 */
fun chatOfNormalTurnEnd(body: ApiEventBody): String? {
    if (body !is ApiEventBody.Agent || !isOrchestratorKey(body.session)) return null
    val ended = body.event as? AgentEvent.TurnEnded ?: return null
    val ok = ended.data as? ResultOfStringOrAgentError.Ok ?: return null
    return if (ok.ok == END_TURN) chatOfOrchestratorKey(body.session) else null
}

// ---- プロジェクトの動作中 (左上のリング、§17.12) -----------------------------------------------------

/** この状態のタスクはエージェント (かマージ) が作業している。 */
private val WORKING_TASK = setOf(TaskStatus.Running, TaskStatus.Merging)

/** 表示中のプロジェクトでターンかタスクの作業が動いている。 */
fun hasRunningWork(view: ProjectView): Boolean {
    if (view.sessions.any { it.status == SessionStatus.Live && it.turnRunning }) return true
    return view.state?.tasks?.any { it.status in WORKING_TASK } ?: false
}

/** 表示中のプロジェクトのチャットにオーケストレータのセッションが上がっている (live)。 */
fun hasLiveOrchestrator(view: ProjectView): Boolean =
    view.sessions.any { it.status == SessionStatus.Live && isOrchestratorKey(it.sessionKey) }

/**
 * 保存されたイベントがそのプロジェクトの作業を始めた・終えたかもしれない: ターンの開始・終了、
 * セッションの起動・停止、タスクの状態の変化。動いているターンの出力 (本文・ツール呼び出し) は違う。
 */
fun changesActivity(ev: ApiEvent): Boolean {
    if (ev.live) return false
    return when (val body = ev.body) {
        is ApiEventBody.Prompted,
        is ApiEventBody.SessionStarted,
        is ApiEventBody.SessionStopped,
        is ApiEventBody.SessionInterrupted,
        -> true
        is ApiEventBody.Agent -> body.event is AgentEvent.TurnEnded || body.event is AgentEvent.Exited
        is ApiEventBody.Domain -> when (body.event) {
            is DomainEvent.TaskCreated,
            is DomainEvent.TaskStatusChanged,
            is DomainEvent.TaskCancelled,
            is DomainEvent.GroupCancelled,
            is DomainEvent.ChatDeleted,
            -> true
            else -> false
        }
        else -> false
    }
}

/**
 * [projects] に [fresh] (コアの一覧) の動作中の情報を取り込む: 知っているプロジェクトの `open` と `running` と
 * `awake` だけ (要求の後に開いたプロジェクトを消さない)。何も変わらなければ同じリスト。
 */
fun mergeActivity(projects: List<ProjectInfo>, fresh: List<ProjectInfo>): List<ProjectInfo> {
    var changed = false
    val merged = projects.map { p ->
        val f = fresh.find { it.id == p.id }
        if (f == null || (f.open == p.open && f.running == p.running && f.awake == p.awake)) {
            p
        } else {
            changed = true
            p.copy(open = f.open, running = f.running, awake = f.awake)
        }
    }
    return if (changed) merged else projects
}

/**
 * [left] (表示していたプロジェクト) の動作中と起動中をそのビューから一覧に書いておく (以後は一覧の値で判断するので)。
 * 読み込み終わっていないビューは何も言わない (コアの値のまま)。
 */
fun rememberRunning(projects: List<ProjectInfo>, left: ProjectView?): List<ProjectInfo> {
    if (left == null || left.phase != LoadPhase.Ready) return projects
    val running = hasRunningWork(left)
    val awake = hasLiveOrchestrator(left)
    if (projects.none { it.id == left.info.id && (it.running != running || it.awake != awake) }) return projects
    return projects.map { if (it.id == left.info.id) it.copy(running = running, awake = awake) else it }
}

/** プロジェクトやチャットのリング。 */
enum class Ring {
    /** 作業が動いている (回る緑)。 */
    Running,

    /** 動いていない (中を抜いた緑)。 */
    Ready,

    /** どのチャットのオーケストレータも上がっていない (グレー)。 */
    Stopped,
}

/**
 * プロジェクトのリング: 作業が動いていれば回る、動いていなければオーケストレータのセッションが上がっていれば
 * 待機、どちらも無ければ停止 (チャットはみな眠っている)。表示中のプロジェクトはビュー、それ以外はコアの最後の値
 * (`running` と `awake`。開いていないプロジェクトにはどちらも立たない) で決める。チャットのリングも同じ眠り方:
 * オーケストレータのプロセスが落ちていて、チャットを使うと戻ってくる。
 */
fun projectRing(p: ProjectInfo, view: ProjectView?): Ring {
    if (view?.info?.id == p.id) {
        if (hasRunningWork(view)) return Ring.Running
        return if (hasLiveOrchestrator(view)) Ring.Ready else Ring.Stopped
    }
    if (p.open && p.running) return Ring.Running
    return if (p.open && p.awake) Ring.Ready else Ring.Stopped
}

/** バックグラウンドで動いているプロジェクト: 開いているが表示していないもの。 */
fun backgroundProjects(projects: List<ProjectInfo>, currentId: String?): List<ProjectInfo> =
    projects.filter { it.open && it.id != currentId }

val RING_LABEL: Map<Ring, String> = mapOf(Ring.Running to "実行中", Ring.Ready to "待機中", Ring.Stopped to "停止")
