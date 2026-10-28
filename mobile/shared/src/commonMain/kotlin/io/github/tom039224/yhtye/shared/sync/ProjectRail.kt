package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo

// プロジェクトのレール (開いているプロジェクトを並べて切り替える細い帯) が使う規則。「開いている」は `ProjectInfo.open`
// (コアのこのプロセスでそのプロジェクトのオーケストレーションが動いている) で、点はリング ([projectRing]) から決める。

/**
 * レールに並べる、開いているプロジェクト: `open` のもの、と表示中のもの ([currentId]。一覧の `open` が追いつく前でも
 * 出す)。並びはコアのまま ([projects] の順。新しく使った順 / ユーザーが並べた順)。
 */
fun openedProjects(projects: List<ProjectInfo>, currentId: String?): List<ProjectInfo> =
    projects.filter { it.open || it.id == currentId }

/** レールの 1 つ: プロジェクトと、その点の状態。 */
data class RailProject(
    val project: ProjectInfo,
    /** 表示中のプロジェクト。 */
    val current: Boolean,
    val ring: Ring,
) {
    /** 作業が動いている (点が回る)。 */
    val working: Boolean get() = ring == Ring.Running
}

/** [openedProjects] を [view] (表示中のプロジェクト) とともにレールの並びに。点は [projectRing]。 */
fun railProjects(projects: List<ProjectInfo>, view: ProjectView?): List<RailProject> {
    val currentId = view?.info?.id
    return openedProjects(projects, currentId).map { RailProject(it, current = it.id == currentId, ring = projectRing(it, view)) }
}

/** 表示中のプロジェクトを含めて、レールのどれかで作業が動いている (レールを閉じていても出すバッジ用)。 */
fun anyWorking(rail: List<RailProject>): Boolean = rail.any { it.working }
