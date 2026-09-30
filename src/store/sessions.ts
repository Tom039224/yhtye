// Agent sessions folded from session events, mirroring how the core derives
// `agent_sessions` (crates/yhtye-core/src/store/sessions.rs).

import type { ApiEventBody, SessionRecord } from "../api/generated";

function update(
  sessions: SessionRecord[],
  key: string,
  f: (s: SessionRecord) => SessionRecord,
): SessionRecord[] {
  return sessions.map((s) => (s.session_key === key ? f(s) : s));
}

export function applySessionEvent(sessions: SessionRecord[], body: ApiEventBody): SessionRecord[] {
  switch (body.type) {
    case "session_started": {
      const row: SessionRecord = {
        session_key: body.session,
        role: body.role,
        task: body.task,
        acp_session_id: body.acp_session_id,
        status: "live",
        turn_running: false,
        // Older logs have no agent (Stage 7b).
        agent: body.agent ?? null,
        // Older logs have no directory (Stage 8).
        cwd: body.cwd ?? null,
      };
      const rest = sessions.filter((s) => s.session_key !== body.session);
      // Same order as the core (SQLite's binary collation).
      return [...rest, row].sort((a, b) => (a.session_key < b.session_key ? -1 : a.session_key > b.session_key ? 1 : 0));
    }
    case "session_stopped":
      return update(sessions, body.session, (s) =>
        body.suspended
          ? { ...s, status: "suspended" }
          : { ...s, status: "stopped", turn_running: false },
      );
    case "session_interrupted":
      return update(sessions, body.session, (s) => ({ ...s, status: "interrupted" }));
    case "prompted":
      return update(sessions, body.session, (s) => ({ ...s, turn_running: true }));
    case "agent":
      return body.event.type === "turn_ended"
        ? update(sessions, body.session, (s) => ({ ...s, turn_running: false }))
        : sessions;
    default:
      return sessions;
  }
}
