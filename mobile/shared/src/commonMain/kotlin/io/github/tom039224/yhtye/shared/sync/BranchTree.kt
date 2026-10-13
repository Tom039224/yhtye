package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ChatInfo
import io.github.tom039224.yhtye.shared.api.generated.BranchState
import io.github.tom039224.yhtye.shared.api.generated.BranchStatuses
import io.github.tom039224.yhtye.shared.api.generated.GitOverview
import io.github.tom039224.yhtye.shared.api.generated.GitWorktree
import io.github.tom039224.yhtye.shared.api.generated.GroupStatus
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus

// 左パネルのブランチ (作業ツリー) とチャットの木 (orchestrator-desktop §9、Stage 8e)。リポジトリの作業ツリー
// (今チェックアウトしているブランチの名前で) とそのチャット (新しい順)、git が一覧に出さなくなった作業ツリーの
// チャット、どの作業ツリーもチェックアウトしていないローカルブランチ。PC の `src/ui/branchTree.ts` の移植。

data class WorktreeNode(
    val path: String,
    /** 呼び名: 今のブランチ、`detached @sha`、git を読むまではディレクトリ名。 */
    val label: String,
    /** メインの作業ツリー (元のクローン)。 */
    val isMain: Boolean,
    /** ディレクトリが無い (そのチャットへの送信は断られる)。 */
    val missing: Boolean,
    val chats: List<ChatInfo>,
)

data class BranchTree(
    val worktrees: List<WorktreeNode>,
    /** git がもう一覧に出さない作業ツリーのチャット (git を読んでから分かる)。 */
    val lost: List<ChatInfo>,
    /** どこにもチェックアウトされていないローカルブランチ (そこへのチャットは Yhtye の作業ツリーを作る)。 */
    val otherBranches: List<String>,
)

/** チャットの作業ツリーの今の場所 (最後に読んだ git による。[Unknown]: まだ読んでいない)。 */
sealed interface Place {
    data class Branch(val name: String) : Place

    data class Detached(val sha: String?) : Place

    data object Missing : Place

    data object Unknown : Place
}

fun findWorktree(overview: GitOverview?, path: String): GitWorktree? = overview?.worktrees?.find { it.path == path }

/**
 * [awaiting]: git を最後に読んだ後に (他のクライアントが、新しい作業ツリーに) 作ったチャット。その読み取りが
 * 作業ツリーを出していなくても、読み直すまでは「見つからない」ではなく不明。
 */
fun placeOf(chat: ChatInfo, overview: GitOverview?, awaiting: Boolean = false): Place {
    if (overview == null) return Place.Unknown
    val w = findWorktree(overview, chat.worktree)
    if (w == null && awaiting) return Place.Unknown
    if (w == null || w.missing) return Place.Missing
    return w.branch?.let { Place.Branch(it) } ?: Place.Detached(w.headSha)
}

/** パスの最後の部分 (`/a/b/feature-x` → `feature-x`)。 */
fun dirName(path: String): String = path.split('/', '\\').lastOrNull { it.isNotEmpty() } ?: path

fun detachedLabel(sha: String?): String = if (sha != null) "detached @${sha.take(7)}" else "detached"

/** ヘッダとピルに出す場所 (git を読むまでは null)。 */
fun placeLabel(place: Place): String? = when (place) {
    is Place.Branch -> place.name
    is Place.Detached -> detachedLabel(place.sha)
    Place.Missing -> "作業ツリーが見つかりません"
    Place.Unknown -> null
}

private fun worktreeLabel(w: GitWorktree): String = w.branch ?: detachedLabel(w.headSha)

/** Yhtye 自身のグループ・タスクの作業ツリー (`yhtye/` で始まるブランチ)。チャットが紐づいているときだけ出す。 */
private fun isShown(w: GitWorktree, chats: List<ChatInfo>): Boolean {
    if (chats.isNotEmpty()) return true
    return !w.missing && !(w.branch != null && isInternalBranch(w.branch))
}

/** git が (まだ) 一覧に出していないチャットの、ディレクトリ名の作業ツリーのノード。 */
private fun unreadNodes(chats: List<ChatInfo>): List<WorktreeNode> =
    chats.map { it.worktree }.distinct().map { path ->
        WorktreeNode(path, dirName(path), isMain = false, missing = false, chats = chats.filter { it.worktree == path })
    }

/**
 * [awaiting]: git を最後に読んだ後に作られたチャット。その読み取りが作業ツリーを出していなければ、
 * 読み直すまでは失われたものではなくディレクトリ名の下に出す。
 */
fun buildTree(chats: List<ChatInfo>, overview: GitOverview?, awaiting: Collection<String> = emptyList()): BranchTree {
    val sorted = byRecency(chats)
    if (overview == null) return BranchTree(unreadNodes(sorted), lost = emptyList(), otherBranches = emptyList())
    val worktrees = overview.worktrees
        .map { w -> w to sorted.filter { it.worktree == w.path } }
        .filter { (w, c) -> isShown(w, c) }
        .map { (w, c) -> WorktreeNode(w.path, worktreeLabel(w), w.isMain, w.missing, c) }
    val listed = overview.worktrees.mapTo(HashSet()) { it.path }
    val checkedOut = overview.worktrees.mapNotNullTo(HashSet()) { it.branch }
    val otherBranches = overview.branches.map { it.name }.filter { !isInternalBranch(it) && it !in checkedOut }
    val unlisted = sorted.filter { it.worktree !in listed }
    val (unread, lost) = unlisted.partition { it.id in awaiting }
    return BranchTree(worktrees + unreadNodes(unread), lost, otherBranches)
}

/** チャットのリング: ターン中は回る、オーケストレータのプロセスが上がっていれば待機、それ以外は停止 (遅延起動)。 */
fun chatRing(view: ProjectView, chat: String): Ring {
    val s = view.sessions.find { it.sessionKey == orchestratorKey(chat) }
    if (s?.status != SessionStatus.Live) return Ring.Stopped
    return if (s.turnRunning) Ring.Running else Ring.Ready
}

/** ブランチの点の日本語ラベル (PR 番号があるときは付ける。PC の `statusLabel` と同じ文言)。 */
fun statusLabel(state: BranchState, pr: Long?): String = when (state) {
    BranchState.Unmerged -> "まだ取り込まれていません"
    BranchState.PrOpen -> if (pr == null) "PR 作成済み" else "PR 作成済み (#$pr)"
    BranchState.Merged -> if (pr == null) "取り込み済み" else "PR でマージ済み (#$pr)"
    BranchState.MergedWithChanges -> if (pr == null) "取り込み済み・未マージの変更あり" else "PR でマージ済み・未マージの変更あり (#$pr)"
    BranchState.Unknown, BranchState.Unrecognized -> "状態不明"
}

/**
 * ブランチの点の状態と PR 番号 (`statuses` に載っていないブランチは `Unknown`。既定のブランチ自
 * 身は点に状態を持たないので null: 呼び出し側は無地の点にする)。
 */
fun statusOf(branch: String?, statuses: BranchStatuses?): Pair<BranchState, Long?>? {
    if (branch == null || branch == statuses?.defaultBranch) return null
    val found = statuses?.statuses?.find { it.branch == branch }
    return (found?.state ?: BranchState.Unknown) to found?.prNumber
}

/** [ms] からの経過 `3m` / `2h` / `4d` (1 分未満と未来は `now`)。 */
fun shortAge(nowMs: Long, ms: Long): String {
    val minutes = (nowMs - ms).floorDiv(60_000L)
    return when {
        minutes < 1 -> "now"
        minutes < 60 -> "${minutes}m"
        minutes < 60 * 24 -> "${minutes / 60}h"
        else -> "${minutes / (60 * 24)}d"
    }
}

/**
 * 今チャットを削除できない理由 (null: できる)。最後はコアが決める。尋ねる前に知らせる: オーケストレータが
 * 動いている、またはグループが終わっていない (`active`, `finishing`, `merge_blocked`)。
 */
fun deleteBlocker(view: ProjectView, chat: String): String? {
    if (chatRing(view, chat) == Ring.Running) return "実行中のため削除できません。先に停止してください。"
    val open = view.state?.groups?.find { it.chat == chat && it.status != GroupStatus.Done && it.status != GroupStatus.Cancelled }
    return open?.let { "グループ ${it.id} が終わっていないため削除できません。完了または中止してください。" }
}
