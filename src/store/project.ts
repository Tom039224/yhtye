// The view of one project: its state (snapshot + folded domain events), agent
// sessions, per-session transcripts and streamed text. Pure functions; the sync
// logic that feeds them lives in app.ts.
//
// Rules (core-design.md §2): durable events are applied once, in `seq` order
// (`cursor` is the last one applied). Domain and session events are folded into
// state only when newer than the snapshot (`stateSeq`); transcripts are built
// from the whole log. Live events only feed `streaming`.

import type { ApiEvent, ProjectInfo, SessionRecord, Snapshot, State } from "../api/generated";
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
    sessions: [],
    transcripts: {},
    streaming: {},
  };
}

/**
 * Starts from a snapshot. Only the last `window` events up to the snapshot are
 * read for the transcripts (the state needs none of them); older history is
 * prepended with `prependHistory` when the user asks for it.
 */
export function applySnapshot(view: ProjectView, snap: Snapshot, window = Number.MAX_SAFE_INTEGER): ProjectView {
  const historyStart = Math.max(1, snap.seq - window + 1);
  return {
    ...view,
    state: snap.state,
    sessions: snap.sessions,
    stateSeq: snap.seq,
    cursor: historyStart - 1,
    historyStart,
  };
}

/** Prepends stored events `[from, historyStart)` (oldest first) to the transcripts. */
export function prependHistory(view: ProjectView, events: ApiEvent[], from: number): ProjectView {
  const page = events.filter((e) => e.seq >= from && e.seq < view.historyStart);
  const older = page.reduce<Transcripts>((t, e) => applyTranscriptEvent(t, e), {});
  return { ...view, transcripts: prependTranscripts(older, view.transcripts), historyStart: from };
}

/** Applies the next durable event (the caller guarantees `seq === cursor + 1`). */
export function applyDurable(view: ProjectView, ev: ApiEvent): ProjectView {
  const next: ProjectView = {
    ...view,
    cursor: ev.seq,
    transcripts: applyTranscriptEvent(view.transcripts, ev),
    streaming: settleStreaming(view.streaming, ev),
  };
  if (ev.seq <= view.stateSeq) return next;
  const body = ev.body;
  if (body.type === "domain" && next.state) {
    return { ...next, state: applyDomainEvent(next.state, body.event) };
  }
  return { ...next, sessions: applySessionEvent(next.sessions, body) };
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
  return { ...view, transcripts: applyTranscriptEvent(view.transcripts, ev) };
}

/** Drops partially streamed text (after a disconnect the chunks are lost). */
export function clearStreaming(view: ProjectView): ProjectView {
  return Object.keys(view.streaming).length === 0 ? view : { ...view, streaming: {} };
}

/** The orchestrator session, if it has been started. */
export function orchestratorSession(view: ProjectView): SessionRecord | undefined {
  return view.sessions.find((s) => s.session_key === "orchestrator");
}
