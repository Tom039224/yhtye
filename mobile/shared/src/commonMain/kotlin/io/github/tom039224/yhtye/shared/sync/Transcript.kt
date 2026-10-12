package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentChoice
import io.github.tom039224.yhtye.shared.api.generated.AgentError
import io.github.tom039224.yhtye.shared.api.generated.AgentEvent
import io.github.tom039224.yhtye.shared.api.generated.AgentOutput
import io.github.tom039224.yhtye.shared.api.generated.ApiEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.DomainEvent
import io.github.tom039224.yhtye.shared.api.generated.InboxKind
import io.github.tom039224.yhtye.shared.api.generated.ResultOfAnyValueOrToolError
import io.github.tom039224.yhtye.shared.api.generated.ResultOfStringOrAgentError
import io.github.tom039224.yhtye.shared.api.generated.State
import io.github.tom039224.yhtye.shared.api.generated.TextKind
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive

// 各セッションが言ったこと・したこと (会話とエージェントの出力の表示用)。durable イベントから畳み込む。
// ストリーミング中の断片 (live) は [Streaming] に別に持ち、まとめられた `agent_text` で置き換える。
// PC の `src/store/transcript.ts` の移植。

/** UI から行ったユーザーの操作のツール呼び出しのセッションキー (`runtime::USER_SESSION`)。 */
const val USER_SESSION: String = "user"

/** 「圧縮」がオーケストレータにそのまま送るプロンプト (`domain::COMPACT_PROMPT`)。 */
const val COMPACT_PROMPT: String = "/compact"

/** 会話・出力の 1 行。[seq] はその元のイベント、[ts] は時刻 (ミリ秒)。 */
sealed interface TranscriptItem {
    val seq: Long
    val ts: Long

    /** ユーザーの発言 (受信箱の `user_message`)。 */
    data class User(override val seq: Long, override val ts: Long, val inboxId: Long, val text: String) : TranscriptItem

    /** 受信箱の通知 (help・グループの完了など。PC の破線の控えめな行)。 */
    data class Notice(
        override val seq: Long,
        override val ts: Long,
        val inboxId: Long,
        val inboxKind: InboxKind,
        val attrs: List<List<String>>,
        val body: String,
    ) : TranscriptItem

    /** サブエージェントへのプロンプト。 */
    data class Prompt(override val seq: Long, override val ts: Long, val text: String) : TranscriptItem

    /** ユーザーがオーケストレータの文脈を圧縮させた (`/compact`)。 */
    data class Compact(override val seq: Long, override val ts: Long) : TranscriptItem

    data class Text(override val seq: Long, override val ts: Long, val textKind: TextKind, val text: String) : TranscriptItem

    /** エージェントのツール呼び出し (ACP)。更新で状態が変わる。 */
    data class Tool(
        override val seq: Long,
        override val ts: Long,
        val id: String,
        val title: String,
        val status: String?,
        val toolKind: String?,
        /** コードとして実行された MCP のツール (OpenCode の `execute`)。例: `yhtye.create_task`。 */
        val calls: List<String>,
    ) : TranscriptItem

    /** Yhtye の MCP ツールの呼び出し (コアの記録)。 */
    data class YhtyeTool(
        override val seq: Long,
        override val ts: Long,
        val tool: String,
        val by: String,
        val ok: Boolean,
        val detail: String,
    ) : TranscriptItem

    /** 会話の中のグループのカード (`group_created`)。 */
    data class GroupCard(override val seq: Long, override val ts: Long, val groupId: String, val title: String) : TranscriptItem

    /** 正常 (`end_turn`) 以外のターンの終わり。 */
    data class Turn(override val seq: Long, override val ts: Long, val outcome: String, val error: Boolean) : TranscriptItem

    /** セッションの起動・停止など。 */
    data class Lifecycle(override val seq: Long, override val ts: Long, val text: String, val error: Boolean) : TranscriptItem
}

/** セッションキー → そのセッションの行。 */
typealias Transcripts = Map<String, List<TranscriptItem>>

private fun Transcripts.push(session: String, item: TranscriptItem): Transcripts =
    this + (session to (this[session].orEmpty() + item))

private fun Transcripts.upsertTool(session: String, seq: Long, ts: Long, update: JsonElement): Transcripts {
    val info = toolCallInfo(update) ?: return this
    val items = this[session].orEmpty()
    val at = items.indexOfFirst { it is TranscriptItem.Tool && it.id == info.id }
    if (at < 0) {
        return push(session, TranscriptItem.Tool(seq, ts, info.id, info.title ?: info.id, info.status, info.kind, info.calls))
    }
    val old = items[at] as TranscriptItem.Tool
    val merged = old.copy(
        title = info.title ?: old.title,
        status = info.status ?: old.status,
        toolKind = info.kind ?: old.toolKind,
        calls = info.calls.ifEmpty { old.calls },
    )
    return this + (session to items.mapIndexed { n, i -> if (n == at) merged else i })
}

private fun choiceText(c: AgentChoice): String = if (c.model.isNullOrEmpty()) c.harness else "${c.harness}/${c.model}"

private const val SHORT_DETAIL_CHARS = 300

private fun short(value: JsonElement): String {
    val text = (value as? JsonPrimitive)?.takeIf { it.isString }?.content ?: value.toString()
    return if (text.length > SHORT_DETAIL_CHARS) "${text.take(SHORT_DETAIL_CHARS)}…" else text
}

/** エージェントのエラーの種類 (`code` タグ)。 */
private fun agentErrorCode(e: AgentError): String = when (e) {
    is AgentError.Spawn -> "spawn"
    is AgentError.Startup -> "startup"
    is AgentError.Timeout -> "timeout"
    is AgentError.Unsupported -> "unsupported"
    is AgentError.Setup -> "setup"
    AgentError.Busy -> "busy"
    AgentError.SteeringUnsupported -> "steering_unsupported"
    is AgentError.Request -> "request"
    AgentError.Closed -> "closed"
    is AgentError.Unrecognized -> e.tag
}

/**
 * durable イベント (保存に失敗したときは live の断片以外) を 1 つ畳み込む。チャットの会話の行は
 * `orchestrator:<chat>` に入る。[state] (グループを知っている状態) でユーザーのツール呼び出しのチャットを決める。
 */
fun applyTranscriptEvent(t: Transcripts, ev: ApiEvent, state: State? = null): Transcripts {
    val seq = ev.seq
    val ts = ev.tsMs
    return when (val body = ev.body) {
        is ApiEventBody.Domain -> when (val event = body.event) {
            // 会話には設計の「GROUP TASK」カードとして出す
            is DomainEvent.GroupCreated ->
                t.push(orchestratorKey(event.group.chat), TranscriptItem.GroupCard(seq, ts, event.group.id, event.group.title))
            is DomainEvent.InboxQueued -> {
                val entry = event.entry
                val item = entry.item
                val row = if (item.kind == InboxKind.UserMessage) {
                    TranscriptItem.User(seq, ts, entry.id, item.body)
                } else {
                    TranscriptItem.Notice(seq, ts, entry.id, item.kind, item.attrs, item.body)
                }
                t.push(orchestratorKey(entry.chat), row)
            }
            else -> t
        }
        // オーケストレータのプロンプトは受信箱のまとめ (ユーザー・通知の行として出す) か、ユーザーの `/compact`
        is ApiEventBody.Prompted -> when {
            !isOrchestratorKey(body.session) -> t.push(body.session, TranscriptItem.Prompt(seq, ts, body.text))
            body.text == COMPACT_PROMPT -> t.push(body.session, TranscriptItem.Compact(seq, ts))
            else -> t
        }
        is ApiEventBody.AgentText -> t.push(body.session, TranscriptItem.Text(seq, ts, body.kind, body.text))
        is ApiEventBody.ToolCalled -> {
            val r = body.record
            // ユーザーの操作は、触れたグループのチャットのもの
            val session = if (r.binding.session == USER_SESSION) {
                chatOfUserCall(state, r.args)?.let(::orchestratorKey)
            } else {
                r.binding.session
            }
            if (session == null) {
                t
            } else {
                val (ok, detail) = when (val result = r.result) {
                    is ResultOfAnyValueOrToolError.Ok -> true to short(result.ok)
                    is ResultOfAnyValueOrToolError.Err -> false to "${result.err.code.wire}: ${result.err.message}"
                    is ResultOfAnyValueOrToolError.Unrecognized -> false to short(result.raw)
                }
                t.push(session, TranscriptItem.YhtyeTool(seq, ts, r.tool, r.binding.session, ok, detail))
            }
        }
        is ApiEventBody.SessionStarted -> {
            // 復元を諦めた起動: どの ACP セッションで会話が失われたかを出す (手で復元できるように)
            val lost = body.lostAcpSessionId
            val text = when {
                lost != null -> "could not restore the previous session ($lost); started a new session"
                body.resumed -> "session restored (session/load)"
                else -> "session started"
            }
            val started = t.push(body.session, TranscriptItem.Lifecycle(seq, ts, text, error = lost != null))
            val replaced = body.replaced
            if (replaced == null) {
                started
            } else {
                val ran = body.agent?.let(::choiceText) ?: "the default"
                val line = "${choiceText(replaced)} is not available (not installed?); started $ran instead"
                started.push(body.session, TranscriptItem.Lifecycle(seq, ts, line, error = true))
            }
        }
        is ApiEventBody.SessionFailed ->
            t.push(body.session, TranscriptItem.Lifecycle(seq, ts, "failed to start: ${body.error}", error = true))
        is ApiEventBody.SessionStopped -> {
            val text = if (body.suspended) "session suspended (Yhtye stopped)" else "session stopped"
            t.push(body.session, TranscriptItem.Lifecycle(seq, ts, text, error = false))
        }
        is ApiEventBody.SessionInterrupted ->
            t.push(body.session, TranscriptItem.Lifecycle(seq, ts, "interrupted by a restart", error = true))
        is ApiEventBody.Agent -> applyAgentEvent(t, body.session, seq, ts, body.event)
        // live の起動中はチャットの「起動中」の印で、アプリ全体のイベントはプロジェクトのものではない
        is ApiEventBody.SessionStarting,
        is ApiEventBody.ProjectsChanged,
        is ApiEventBody.SettingsChanged,
        ApiEventBody.Resync,
        is ApiEventBody.RemoteStatusChanged,
        is ApiEventBody.RemoteDevicesChanged,
        is ApiEventBody.ClientsChanged,
        is ApiEventBody.Unrecognized,
        -> t
    }
}

private fun applyAgentEvent(t: Transcripts, session: String, seq: Long, ts: Long, event: AgentEvent): Transcripts =
    when (event) {
        is AgentEvent.Output -> when (val out = event.data) {
            is AgentOutput.ToolCall -> t.upsertTool(session, seq, ts, out.update)
            is AgentOutput.ToolCallUpdate -> t.upsertTool(session, seq, ts, out.update)
            else -> t
        }
        is AgentEvent.TurnEnded -> when (val data = event.data) {
            is ResultOfStringOrAgentError.Err ->
                t.push(session, TranscriptItem.Turn(seq, ts, "error (${agentErrorCode(data.err)})", error = true))
            is ResultOfStringOrAgentError.Ok ->
                if (data.ok == END_TURN) t else t.push(session, TranscriptItem.Turn(seq, ts, data.ok, error = data.ok != CANCELLED))
            is ResultOfStringOrAgentError.Unrecognized ->
                t.push(session, TranscriptItem.Turn(seq, ts, "error (${data.raw})", error = true))
        }
        is AgentEvent.Exited -> {
            val text = "process exited (code ${event.data.code ?: "-"}, signal ${event.data.signal ?: "-"})"
            t.push(session, TranscriptItem.Lifecycle(seq, ts, text, error = false))
        }
        is AgentEvent.PermissionAutoAnswered -> {
            val chosen = event.data.chosen
            t.push(session, TranscriptItem.Lifecycle(seq, ts, "permission auto-answered: ${chosen ?: "cancelled"}", error = chosen == null))
        }
        else -> t
    }

/** ACP の正常なターンの終わり (`StopReason::EndTurn`)。 */
const val END_TURN: String = "end_turn"

/** ユーザー (クライアント) が中止したターンの終わり。 */
const val CANCELLED: String = "cancelled"

/** ストリーミング中の本文 (最後の `agent_text` 以降の live の断片)。 */
data class Streamed(val message: String = "", val thought: String = "")

/** セッションキー → ストリーミング中の本文。 */
typealias Streaming = Map<String, Streamed>

/** live の断片を足す。本文の断片のイベントでなければ null。 */
fun applyChunk(s: Streaming, ev: ApiEvent): Streaming? {
    val body = ev.body as? ApiEventBody.Agent ?: return null
    val out = (body.event as? AgentEvent.Output)?.data ?: return null
    val (update, isMessage) = when (out) {
        is AgentOutput.MessageChunk -> out.update to true
        is AgentOutput.ThoughtChunk -> out.update to false
        else -> return null
    }
    val text = chunkText(update) ?: return s
    val cur = s[body.session] ?: Streamed()
    val next = if (isMessage) cur.copy(message = cur.message + text) else cur.copy(thought = cur.thought + text)
    return s + (body.session to next)
}

/** durable イベントが完成させた・終わらせたストリーミングの本文を消す。 */
fun settleStreaming(s: Streaming, ev: ApiEvent): Streaming {
    val body = ev.body
    val (session, clearMessage, clearThought) = when {
        body is ApiEventBody.AgentText -> Triple(body.session, body.kind != TextKind.Thought, body.kind != TextKind.Message)
        body is ApiEventBody.SessionStopped -> Triple(body.session, true, true)
        body is ApiEventBody.Agent && (body.event is AgentEvent.TurnEnded || body.event is AgentEvent.Exited) ->
            Triple(body.session, true, true)
        else -> return s
    }
    val cur = s[session] ?: return s
    val next = Streamed(
        message = if (clearMessage) "" else cur.message,
        thought = if (clearThought) "" else cur.thought,
    )
    return if (next == cur) s else s + (session to next)
}

/**
 * 前のページの履歴 (単独で畳み込んだもの) を読み込み済みの前に足す。前のページで始まり後のページで
 * 更新されたツール呼び出しは、始まった位置に 1 つだけ (後の状態で) 出す。
 */
fun prependTranscripts(older: Transcripts, newer: Transcripts): Transcripts {
    val out = newer.toMutableMap()
    for ((session, before) in older) {
        val after = newer[session].orEmpty()
        val updates = after.filterIsInstance<TranscriptItem.Tool>().associateBy { it.id }
        val merged = before.map { item ->
            val update = (item as? TranscriptItem.Tool)?.let { updates[it.id] } ?: return@map item
            // 更新だけの行は ID が題になっている: そのときは本当の題を残す
            val title = if (update.title != update.id) update.title else item.title
            item.copy(
                title = title,
                status = update.status ?: item.status,
                toolKind = update.toolKind ?: item.toolKind,
                calls = update.calls.ifEmpty { item.calls },
            )
        }
        val moved = before.mapNotNullTo(HashSet()) { i -> (i as? TranscriptItem.Tool)?.id?.takeIf { it in updates } }
        out[session] = merged + after.filterNot { it is TranscriptItem.Tool && it.id in moved }
    }
    return out
}
