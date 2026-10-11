package io.github.tom039224.yhtye.shared.sync

import io.github.tom039224.yhtye.shared.api.generated.AgentEvent
import io.github.tom039224.yhtye.shared.api.generated.ApiEventBody
import io.github.tom039224.yhtye.shared.api.generated.SessionRecord
import io.github.tom039224.yhtye.shared.api.generated.SessionStatus

// セッションの表 (`agent_sessions`) をセッションのイベントから畳み込む。コアの導き方
// (crates/yhtye-core/src/store/sessions.rs) と PC の `src/store/sessions.ts` の移植。

private inline fun List<SessionRecord>.update(key: String, f: (SessionRecord) -> SessionRecord): List<SessionRecord> =
    if (none { it.sessionKey == key }) this else map { if (it.sessionKey == key) f(it) else it }

fun applySessionEvent(sessions: List<SessionRecord>, body: ApiEventBody): List<SessionRecord> = when (body) {
    is ApiEventBody.SessionStarted -> {
        // 以前に失われたセッションは、別のものが失われるまで記録したまま (コアと同じ)
        val lost = body.lostAcpSessionId ?: sessions.find { it.sessionKey == body.session }?.lostAcpSessionId
        val row = SessionRecord(
            sessionKey = body.session,
            role = body.role,
            task = body.task,
            acpSessionId = body.acpSessionId,
            status = SessionStatus.Live,
            turnRunning = false,
            agent = body.agent,
            cwd = body.cwd,
            lostAcpSessionId = lost,
        )
        // コアと同じ順 (SQLite のバイナリ照合 = UTF-8 のバイト順。キーは ASCII なので文字の順と同じ)
        (sessions.filter { it.sessionKey != body.session } + row).sortedBy { it.sessionKey }
    }
    is ApiEventBody.SessionStopped -> sessions.update(body.session) {
        if (body.suspended) it.copy(status = SessionStatus.Suspended) else it.copy(status = SessionStatus.Stopped, turnRunning = false)
    }
    is ApiEventBody.SessionInterrupted -> sessions.update(body.session) { it.copy(status = SessionStatus.Interrupted) }
    is ApiEventBody.Prompted -> sessions.update(body.session) { it.copy(turnRunning = true) }
    is ApiEventBody.Agent ->
        if (body.event is AgentEvent.TurnEnded) sessions.update(body.session) { it.copy(turnRunning = false) } else sessions
    else -> sessions
}
