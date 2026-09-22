// Builders for constructed events in tests.

import type { ApiEvent, ApiEventBody } from "../api/generated";

export function ev(seq: number, body: ApiEventBody, live = false, project = "repo"): ApiEvent {
  return { seq, ts_ms: 1_700_000_000_000 + seq, project, live, body };
}

export function chunk(seq: number, session: string, text: string, kind: "message_chunk" | "thought_chunk" = "message_chunk"): ApiEvent {
  return ev(seq, { type: "agent", session, event: { type: "output", data: { kind, update: { content: { type: "text", text } } } } }, true);
}

export function agentText(seq: number, session: string, text: string, kind: "message" | "thought" = "message"): ApiEvent {
  return ev(seq, { type: "agent_text", session, kind, text });
}

export function turnEnded(seq: number, session: string, reason = "end_turn"): ApiEvent {
  return ev(seq, { type: "agent", session, event: { type: "turn_ended", data: { Ok: reason } } });
}

export function prompted(seq: number, session: string, text = "p"): ApiEvent {
  return ev(seq, { type: "prompted", session, text });
}

export function userMessage(seq: number, inboxId: number, text: string): ApiEvent {
  return ev(seq, { type: "domain", event: { type: "inbox_queued", entry: { id: inboxId, item: { kind: "user_message", attrs: [], body: text } } } });
}

export function delivered(seq: number, upTo: number): ApiEvent {
  return ev(seq, { type: "domain", event: { type: "inbox_delivered", up_to: upTo } });
}
