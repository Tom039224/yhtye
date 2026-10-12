package io.github.tom039224.yhtye.shared.sync

// タスクのエージェントへオーケストレータの `message_task` のメッセージを届けるプロンプト
// (yhtye-core の `prompts::orchestrator_message_prompt`):
//
//   [yhtye:orchestrator_message]
//   (Your previous turn was interrupted by Yhtye ...)           ← 割り込んで送り直したときだけ
//   <オーケストレータの本文>
//
//   (This message is from the orchestrator while you are ...)   ← エージェント向けの注記
//
// 会話には本文だけを「オーケストレータから」として出し、注記は出さない。
// PC の `src/store/orchestratorMessage.ts` と同じ読み方 (テストの事例も同じ)。

/** プロンプトの 1 行目 (`prompts::ORCHESTRATOR_MESSAGE_HEADER`)。 */
const val ORCHESTRATOR_MESSAGE_HEADER: String = "[yhtye:orchestrator_message]"

/** 割り込みの前置きの行の頭。頭だけを見るので、後ろの文言が変わっても読める。 */
private const val INTERRUPT_NOTE_START = "(Your previous turn was interrupted by Yhtye"

/** 末尾のエージェント向けの注記の頭。同じく頭だけを見る。 */
private const val AGENT_NOTE_START = "(This message is from the orchestrator"

/**
 * オーケストレータのメッセージ。[text] は本文 (Markdown。Yhtye が前後に足した行は除く)、
 * [interrupted] は届けるために Yhtye がエージェントのターンを止めた (ステアリング非対応のハーネス)。
 */
data class OrchestratorMessage(val text: String, val interrupted: Boolean)

/** [prompt] がオーケストレータのメッセージならその中身、そうでなければ null (ほかのプロンプトと同じ表示になる)。 */
fun parseOrchestratorMessage(prompt: String): OrchestratorMessage? {
    val lines = prompt.replace("\r\n", "\n").split("\n")
    if (lines[0].trimEnd() != ORCHESTRATOR_MESSAGE_HEADER) return null
    val interrupted = lines.getOrNull(1)?.startsWith(INTERRUPT_NOTE_START) == true
    val rest = lines.drop(if (interrupted) 2 else 1).joinToString("\n").trim()
    // 注記は最後の段落。本文が注記を引用していることもあるので、最後の段落だけを見る
    val gap = rest.lastIndexOf("\n\n")
    val noteAt = if (gap < 0) 0 else gap + 2
    val text = (if (rest.substring(noteAt).startsWith(AGENT_NOTE_START)) rest.substring(0, noteAt) else rest).trim()
    return if (text.isEmpty()) null else OrchestratorMessage(text, interrupted)
}
