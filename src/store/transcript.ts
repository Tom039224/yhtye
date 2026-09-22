// What each session said and did, folded from durable events, for the
// conversation and agent output views. Streaming chunks (live events) are kept
// separately in `Streaming` and replaced by the coalesced `agent_text`.

import { chunkText, toolCallInfo } from "../api/acp";
import type { ApiEvent, InboxKind, TextKind } from "../api/generated";

export const ORCHESTRATOR = "orchestrator";
/** Session key of tool calls the user made through the UI (runtime::USER_SESSION). */
export const USER = "user";

interface Base {
  seq: number;
  ts: number;
}

export type TranscriptItem =
  | (Base & { kind: "user"; inboxId: number; text: string })
  | (Base & { kind: "notice"; inboxId: number; inboxKind: InboxKind; attrs: [string, string][]; body: string })
  | (Base & { kind: "prompt"; text: string })
  | (Base & { kind: "text"; textKind: TextKind; text: string })
  | (Base & { kind: "tool"; id: string; title: string; status: string | null; toolKind: string | null })
  | (Base & { kind: "yhtye_tool"; tool: string; by: string; ok: boolean; detail: string })
  | (Base & { kind: "group"; groupId: string; title: string })
  | (Base & { kind: "turn"; outcome: string; error: boolean })
  | (Base & { kind: "lifecycle"; text: string; error: boolean });

export type Transcripts = Record<string, TranscriptItem[]>;

function push(t: Transcripts, session: string, item: TranscriptItem): Transcripts {
  return { ...t, [session]: [...(t[session] ?? []), item] };
}

function upsertTool(t: Transcripts, session: string, base: Base, update: unknown): Transcripts {
  const info = toolCallInfo(update);
  if (!info) return t;
  const items = t[session] ?? [];
  const at = items.findIndex((i) => i.kind === "tool" && i.id === info.id);
  if (at < 0) {
    const item: TranscriptItem = {
      ...base,
      kind: "tool",
      id: info.id,
      title: info.title ?? info.id,
      status: info.status,
      toolKind: info.kind,
    };
    return push(t, session, item);
  }
  const old = items[at];
  if (old.kind !== "tool") return t;
  const merged: TranscriptItem = {
    ...old,
    title: info.title ?? old.title,
    status: info.status ?? old.status,
    toolKind: info.kind ?? old.toolKind,
  };
  return { ...t, [session]: items.map((i, n) => (n === at ? merged : i)) };
}

function short(value: unknown): string {
  const text = typeof value === "string" ? value : JSON.stringify(value);
  return text.length > 300 ? `${text.slice(0, 300)}…` : text;
}

/** Folds one durable (or, when storing failed, live non-chunk) event. */
export function applyTranscriptEvent(t: Transcripts, ev: ApiEvent): Transcripts {
  const base: Base = { seq: ev.seq, ts: ev.ts_ms };
  const body = ev.body;
  switch (body.type) {
    case "domain": {
      if (body.event.type === "group_created") {
        // Shown in the conversation as the design's "GROUP TASK" card (§3.4).
        const g = body.event.group;
        return push(t, ORCHESTRATOR, { ...base, kind: "group", groupId: g.id, title: g.title });
      }
      if (body.event.type !== "inbox_queued") return t;
      const { id, item } = body.event.entry;
      return item.kind === "user_message"
        ? push(t, ORCHESTRATOR, { ...base, kind: "user", inboxId: id, text: item.body })
        : push(t, ORCHESTRATOR, {
            ...base,
            kind: "notice",
            inboxId: id,
            inboxKind: item.kind,
            attrs: item.attrs,
            body: item.body,
          });
    }
    case "prompted":
      // The orchestrator's prompts are its inbox batches, shown as user / notice items.
      return body.session === ORCHESTRATOR
        ? t
        : push(t, body.session, { ...base, kind: "prompt", text: body.text });
    case "agent_text":
      return push(t, body.session, { ...base, kind: "text", textKind: body.kind, text: body.text });
    case "tool_called": {
      const r = body.record;
      const session = r.binding.session === USER ? ORCHESTRATOR : r.binding.session;
      const ok = "Ok" in r.result;
      const detail = "Ok" in r.result ? short(r.result.Ok) : `${r.result.Err.code}: ${r.result.Err.message}`;
      return push(t, session, { ...base, kind: "yhtye_tool", tool: r.tool, by: r.binding.session, ok, detail });
    }
    case "session_started":
      return push(t, body.session, {
        ...base,
        kind: "lifecycle",
        text: body.resumed ? "session restored (session/load)" : "session started",
        error: false,
      });
    case "session_failed":
      return push(t, body.session, { ...base, kind: "lifecycle", text: `failed to start: ${body.error}`, error: true });
    case "session_stopped":
      return push(t, body.session, {
        ...base,
        kind: "lifecycle",
        text: body.suspended ? "session suspended (Yhtye stopped)" : "session stopped",
        error: false,
      });
    case "session_interrupted":
      return push(t, body.session, { ...base, kind: "lifecycle", text: "interrupted by a restart", error: true });
    case "agent":
      return applyAgentEvent(t, body.session, base, body.event);
  }
}

function applyAgentEvent(
  t: Transcripts,
  session: string,
  base: Base,
  event: Extract<ApiEvent["body"], { type: "agent" }>["event"],
): Transcripts {
  switch (event.type) {
    case "output": {
      const out = event.data;
      if (out.kind === "tool_call" || out.kind === "tool_call_update") {
        return upsertTool(t, session, base, out.update);
      }
      return t;
    }
    case "turn_ended": {
      if ("Err" in event.data) {
        const e = event.data.Err;
        return push(t, session, { ...base, kind: "turn", outcome: `error (${e.code})`, error: true });
      }
      const reason = event.data.Ok;
      return reason === "end_turn" ? t : push(t, session, { ...base, kind: "turn", outcome: reason, error: reason !== "cancelled" });
    }
    case "exited": {
      const { code, signal } = event.data;
      const text = `process exited (code ${code ?? "-"}, signal ${signal ?? "-"})`;
      return push(t, session, { ...base, kind: "lifecycle", text, error: false });
    }
    case "permission_auto_answered":
      return push(t, session, {
        ...base,
        kind: "lifecycle",
        text: `permission auto-answered: ${event.data.chosen ?? "cancelled"}`,
        error: event.data.chosen === null,
      });
    default:
      return t;
  }
}

/** Text being streamed per session (live chunks since the last `agent_text`). */
export type Streaming = Record<string, { message: string; thought: string }>;

/** Adds a live chunk; `null` if the event is not a text chunk. */
export function applyChunk(s: Streaming, ev: ApiEvent): Streaming | null {
  const body = ev.body;
  if (body.type !== "agent" || body.event.type !== "output") return null;
  const out = body.event.data;
  if (out.kind !== "message_chunk" && out.kind !== "thought_chunk") return null;
  const text = chunkText(out.update);
  if (text === null) return s;
  const cur = s[body.session] ?? { message: "", thought: "" };
  const next =
    out.kind === "message_chunk"
      ? { ...cur, message: cur.message + text }
      : { ...cur, thought: cur.thought + text };
  return { ...s, [body.session]: next };
}

/** Clears streamed text that a durable event completes or ends. */
export function settleStreaming(s: Streaming, ev: ApiEvent): Streaming {
  const body = ev.body;
  let session: string;
  let clear: "message" | "thought" | "both";
  if (body.type === "agent_text") {
    session = body.session;
    clear = body.kind;
  } else if (body.type === "session_stopped") {
    session = body.session;
    clear = "both";
  } else if (body.type === "agent" && (body.event.type === "turn_ended" || body.event.type === "exited")) {
    session = body.session;
    clear = "both";
  } else {
    return s;
  }
  const cur = s[session];
  if (!cur) return s;
  const next = {
    message: clear === "thought" ? cur.message : "",
    thought: clear === "message" ? cur.thought : "",
  };
  return next.message === cur.message && next.thought === cur.thought ? s : { ...s, [session]: next };
}

/**
 * Puts an older page of history (folded on its own) in front of what is
 * loaded. A tool call started in the older page and updated in the newer one
 * appears once, at its start, with the newer status.
 */
export function prependTranscripts(older: Transcripts, newer: Transcripts): Transcripts {
  const out: Transcripts = { ...newer };
  for (const [session, before] of Object.entries(older)) {
    const after = newer[session] ?? [];
    const updates = new Map<string, Extract<TranscriptItem, { kind: "tool" }>>();
    for (const item of after) if (item.kind === "tool") updates.set(item.id, item);
    const merged = before.map((item) => {
      if (item.kind !== "tool") return item;
      const update = updates.get(item.id);
      if (!update) return item;
      // An update-only item is titled with its id; keep the real title then.
      const title = update.title !== update.id ? update.title : item.title;
      return { ...item, title, status: update.status ?? item.status, toolKind: update.toolKind ?? item.toolKind };
    });
    const moved = new Set(before.flatMap((i) => (i.kind === "tool" && updates.has(i.id) ? [i.id] : [])));
    out[session] = [...merged, ...after.filter((i) => !(i.kind === "tool" && moved.has(i.id)))];
  }
  return out;
}
