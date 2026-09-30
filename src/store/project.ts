// The view of one project: its state (snapshot + folded domain events), agent
// sessions, per-session transcripts and streamed text. Pure functions; the sync
// logic that feeds them lives in app.ts.
//
// Rules (core-design.md §2): durable events are applied once, in `seq` order
// (`cursor` is the last one applied). Domain and session events are folded into
// state only when newer than the snapshot (`stateSeq`); transcripts are built
// from the whole log. Live events only feed `streaming`.

import type { ApiEvent, ChatInfo, DomainEvent, ProjectInfo, SessionRecord, Snapshot, State } from "../api/generated";
import { applyChatEvent, initialChat, orchestratorKey } from "./chats";
import { applyDomainEvent } from "./domain";
import { applySessionEvent } from "./sessions";
import {
  applyChunk,
  applyTranscriptEvent,
  prependTranscripts,
  settleStreaming,
  type Streaming,
  type Transcripts,
} from "./transcript";

export interface ProjectView {
  info: ProjectInfo;
  phase: "loading" | "ready" | "error";
  loadError: string | null;
  /** `seq` of the snapshot the state started from. */
  stateSeq: number;
  /** `seq` of the last durable event applied. */
  cursor: number;
  /**
   * Lowest `seq` whose event is in the transcripts: history is loaded from the
   * most recent events backwards on demand (1 = the whole log is loaded).
   */
  historyStart: number;
  /** An older page of history is being loaded. */
  loadingOlder: boolean;
  state: State | null;
  /** The project's chats (`Snapshot.chats`, then folded from events). */
  chats: ChatInfo[];
  /** The chat whose conversation and groups are shown (`null`: the project has none). */
  selectedChat: string | null;
  /** Chats that got a notification while another chat was selected. */
  unread: Record<string, true>;
  sessions: SessionRecord[];
  transcripts: Transcripts;
  streaming: Streaming;
}

export function newProjectView(info: ProjectInfo): ProjectView {
  return {
    info,
    phase: "loading",
    loadError: null,
    stateSeq: 0,
    cursor: 0,
    historyStart: 1,
    loadingOlder: false,
    state: null,
    chats: [],
    selectedChat: null,
    unread: {},
    sessions: [],
    transcripts: {},
    streaming: {},
  };
}

/**
 * Starts from a snapshot. Only the last `window` events up to the snapshot are
 * read for the transcripts (the state needs none of them); older history is
 * prepended with `prependHistory` when the user asks for it. The chat shown is
 * `remembered` if it exists, else the last used one.
 */
export function applySnapshot(
  view: ProjectView,
  snap: Snapshot,
  window = Number.MAX_SAFE_INTEGER,
  remembered: string | null = null,
): ProjectView {
  const historyStart = Math.max(1, snap.seq - window + 1);
  return {
    ...view,
    state: snap.state,
    chats: snap.chats,
    selectedChat: initialChat(snap.chats, remembered),
    unread: {},
    sessions: snap.sessions,
    stateSeq: snap.seq,
    cursor: historyStart - 1,
    historyStart,
  };
}

/** Prepends stored events `[from, historyStart)` (oldest first) to the transcripts. */
export function prependHistory(view: ProjectView, events: ApiEvent[], from: number): ProjectView {
  const page = events.filter((e) => e.seq >= from && e.seq < view.historyStart);
  const older = page.reduce<Transcripts>((t, e) => applyTranscriptEvent(t, e, view.state), {});
  return { ...view, transcripts: prependTranscripts(older, view.transcripts), historyStart: from };
}

/** Applies the next durable event (the caller guarantees `seq === cursor + 1`). */
export function applyDurable(view: ProjectView, ev: ApiEvent): ProjectView {
  const next: ProjectView = {
    ...view,
    cursor: ev.seq,
    transcripts: applyTranscriptEvent(view.transcripts, ev, view.state),
    streaming: settleStreaming(view.streaming, ev),
  };
  if (ev.seq <= view.stateSeq) return next;
  const body = ev.body;
  const chats = applyChatEvent(next.chats, ev);
  if (body.type === "domain" && next.state) {
    const unread = markUnread(next, body.event);
    return { ...next, chats, unread, state: applyDomainEvent(next.state, body.event) };
  }
  return { ...next, chats, sessions: applySessionEvent(next.sessions, body) };
}

/** A notification for a chat that is not on screen marks it (cleared by selecting it). */
function markUnread(view: ProjectView, event: DomainEvent): ProjectView["unread"] {
  if (event.type !== "inbox_queued") return view.unread;
  const { chat, item } = event.entry;
  if (item.kind === "user_message" || chat === view.selectedChat || view.unread[chat]) return view.unread;
  return { ...view.unread, [chat]: true };
}

/**
 * A live event: text chunks are streamed; anything else live was not stored
 * (the core falls back to live when saving fails), so it is shown but not folded.
 */
export function applyLive(view: ProjectView, ev: ApiEvent): ProjectView {
  const streaming = applyChunk(view.streaming, ev);
  if (streaming) return streaming === view.streaming ? view : { ...view, streaming };
  const body = ev.body;
  if (body.type === "agent" && body.event.type === "output" && body.event.data.kind === "usage") {
    return view;
  }
  return { ...view, transcripts: applyTranscriptEvent(view.transcripts, ev, view.state) };
}

/** Drops partially streamed text (after a disconnect the chunks are lost). */
export function clearStreaming(view: ProjectView): ProjectView {
  return Object.keys(view.streaming).length === 0 ? view : { ...view, streaming: {} };
}

/** A chat's orchestrator session, if it has been started. */
export function orchestratorSession(view: ProjectView, chat: string | null): SessionRecord | undefined {
  return chat ? view.sessions.find((s) => s.session_key === orchestratorKey(chat)) : undefined;
}

/** The selected chat's list entry. */
export function selectedChatInfo(view: ProjectView): ChatInfo | undefined {
  return view.chats.find((c) => c.id === view.selectedChat);
}
