package io.github.tom039224.yhtye.shared.sync

// グループとタスクの ID (orchestration-model §2.1): オーケストレータが付ける名前 (`headless-server`、`remote-icon`)。
// 古いプロジェクトの連番 (`G-<n>`、`T-<n>`) もそのまま有効。コアはブランチをすべてそこから作る
// (`domain::task_branch`)。ここはブランチ名から読み戻す。PC の `src/store/names.ts` の移植。

/** グループ・タスクの名前の長さの上限 (コアの `MAX_NAME_CHARS`)。 */
const val MAX_NAME_CHARS: Int = 32

private val NAME = Regex("^[a-z0-9]+(-[a-z0-9]+)*$")
private val NUMBERED_GROUP = Regex("^G-\\d+$")
private val NUMBERED_TASK = Regex("^T-\\d+$")

/** 古いタスクのブランチ `yhtye/<group>-T-<n>`: 名前は小文字だけなので、末尾の `-T-<n>` が名前の一部になることはない。 */
private val NUMBERED_TASK_SUFFIX = Regex("^(.+)-(T-\\d+)$")

/** タスクブランチのグループとタスクの区切り (名前には含まれない)。 */
private const val TASK_SEPARATOR = "--"

/** 名前: `^[a-z0-9]+(-[a-z0-9]+)*$`、32 文字まで。 */
fun isName(id: String): Boolean = id.length <= MAX_NAME_CHARS && NAME.matches(id)

fun isGroupId(id: String): Boolean = isName(id) || NUMBERED_GROUP.matches(id)

fun isTaskId(id: String): Boolean = isName(id) || NUMBERED_TASK.matches(id)

/** Yhtye のブランチが何のものか。 */
sealed interface YhtyeBranch {
    val group: String

    data class Group(override val group: String) : YhtyeBranch

    data class Task(override val group: String, val task: String) : YhtyeBranch
}

/**
 * グループ・タスクのブランチを読む (それ以外は null): `yhtye/<group>`、`yhtye/<group>--<task>`、古い
 * `yhtye/G-<n>-T-<m>` (古いグループの名前のタスク `yhtye/G-46--remote-icon` も)。
 */
fun parseYhtyeBranch(branch: String): YhtyeBranch? {
    if (!branch.startsWith(INTERNAL_BRANCH_PREFIX)) return null
    val rest = branch.removePrefix(INTERNAL_BRANCH_PREFIX)
    val split = rest.indexOf(TASK_SEPARATOR)
    if (split >= 0) {
        val group = rest.substring(0, split)
        val task = rest.substring(split + TASK_SEPARATOR.length)
        return if (isGroupId(group) && isName(task)) YhtyeBranch.Task(group, task) else null
    }
    NUMBERED_TASK_SUFFIX.matchEntire(rest)?.let { m ->
        val (group, task) = m.destructured
        if (isGroupId(group)) return YhtyeBranch.Task(group, task)
    }
    return if (isGroupId(rest)) YhtyeBranch.Group(rest) else null
}
