package io.github.tom039224.yhtye.ui.conversation

import androidx.compose.runtime.Stable
import androidx.compose.runtime.mutableStateMapOf

// タスクの引用 (PC の `src/ui/mentions.ts`)。PC ではタスクカードの `@`、スマホでは右パネルのタスクの行を右→左に
// スワイプしてコンポーザに積む (T-69)。オーケストレータはタスクを ID で知っているので、送る文の先頭に
// `@T-n 題` を 1 行ずつ付ける。

data class Mention(val task: String, val title: String)

/** [m] を足す (同じタスクがすでにあれば何もしない)。 */
fun addMention(list: List<Mention>, m: Mention): List<Mention> = if (list.any { it.task == m.task }) list else list + m

fun removeMention(list: List<Mention>, task: String): List<Mention> = list.filter { it.task != task }

/** オーケストレータに送る文: 引用の行、空行、本文。 */
fun withMentions(text: String, mentions: List<Mention>): String {
    if (mentions.isEmpty()) return text
    return mentions.joinToString("\n") { "@${it.task} ${it.title}" } + "\n\n" + text
}

/**
 * チャットごとのメンション (PC と同じく、積んだチャットのコンポーザに属する)。画面の作り直しでも残すため
 * [io.github.tom039224.yhtye.ui.MainViewModel] が持つ。
 */
@Stable
class MentionsState {
    private val byChat = mutableStateMapOf<String, List<Mention>>()

    fun of(chat: String): List<Mention> = byChat[chat].orEmpty()

    /** [chat] のコンポーザに [mention] を積む (T-69 のタスクの行のスワイプ・右パネルの `@` から)。 */
    fun add(chat: String, mention: Mention) {
        byChat[chat] = addMention(of(chat), mention)
    }

    fun remove(chat: String, task: String) {
        byChat[chat] = removeMention(of(chat), task)
    }

    fun clear(chat: String) {
        byChat.remove(chat)
    }
}
