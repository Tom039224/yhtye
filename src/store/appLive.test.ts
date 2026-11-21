// How the store shows live events (streamed text, usage): collected, and shown
// once per frame in a single notification, never out of order with stored events,
// and without touching `AppState.project` (so the screen is not rendered again).

import { beforeEach, describe, expect, it, vi } from "vitest";

import { agentText, chunk, ev, ORCHESTRATOR, prompted, usage } from "../test/events";
import { durable, FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { AppStore } from "./app";
import { LIVE_FLUSH_MAX_MS, type LiveScheduler } from "./liveBatch";

const LOG = durable(FULL_RUN);
const LAST = LOG[LOG.length - 1].seq;

let transport: MemoryTransport;
let core: FakeCore;
/** The frames the store asked for (run one with `frame()`). */
let frames: (() => void)[];
let store: AppStore;
let notifications: number;

const scheduler: LiveScheduler = (flush) => {
  frames.push(flush);
  return () => {
    frames = frames.filter((f) => f !== flush);
  };
};

/** The next animation frame comes. */
function frame(): void {
  const next = frames.shift();
  if (!next) throw new Error("no frame was asked for");
  next();
}

const project = () => {
  const p = store.getState().project;
  if (!p) throw new Error("no project");
  return p;
};
const text = (session = ORCHESTRATOR) => store.getState().live.streaming[session]?.message;

async function open() {
  await store.openProject(PROJECT.path);
  await vi.waitFor(() => expect(project().phase).toBe("ready"));
  store.subscribe(() => notifications++);
  notifications = 0;
}

beforeEach(async () => {
  frames = [];
  notifications = 0;
  transport = new MemoryTransport();
  core = new FakeCore(PROJECT, FULL_RUN.start);
  core.log = LOG;
  core.attach(transport);
  store = new AppStore(transport, { liveScheduler: scheduler });
  store.start();
  await open();
});

describe("live events", () => {
  it("are shown in one notification at the next frame, however many chunks came", () => {
    for (const piece of ["Hel", "lo ", "wor", "ld"]) transport.emit(chunk(LAST, ORCHESTRATOR, piece));
    transport.emit(chunk(LAST, "T-1/implementer", "other"));
    expect(notifications).toBe(0);
    expect(text()).toBeUndefined();
    expect(frames).toHaveLength(1);

    frame();
    expect(notifications).toBe(1);
    expect(text()).toBe("Hello world");
    expect(text("T-1/implementer")).toBe("other");
  });

  it("leave the project view alone: a chunk renders the screen only where its text is shown", () => {
    const before = project();
    const sessions = before.sessions;
    transport.emit(chunk(LAST, "T-1/implementer", "work"));
    transport.emit(usage(LAST, ORCHESTRATOR, { used: 1_000, size: 200_000 }));
    frame();
    expect(project()).toBe(before);
    expect(project().sessions).toBe(sessions);
    expect(store.getState().live.contextUsage[ORCHESTRATOR]).toMatchObject({ used: 1_000 });
  });

  it("keep the text of the sessions a chunk is not for as it was", () => {
    transport.emit(chunk(LAST, ORCHESTRATOR, "mine"));
    frame();
    const mine = store.getState().live.streaming[ORCHESTRATOR];
    transport.emit(chunk(LAST, "T-1/implementer", "theirs"));
    frame();
    expect(store.getState().live.streaming[ORCHESTRATOR]).toBe(mine);
  });

  it("are shown before the stored event that follows them is applied, not after", () => {
    transport.emit(chunk(LAST, ORCHESTRATOR, "Hi "));
    transport.emit(chunk(LAST, ORCHESTRATOR, "there"));
    transport.emit(agentText(LAST + 1, ORCHESTRATOR, "Hi there")); // settles what was streamed
    // The chunks were applied first, so the stored text replaced them: nothing is left to show twice.
    expect(text()).toBe("");
    expect(project().cursor).toBe(LAST + 1);
    expect(project().transcripts[ORCHESTRATOR].at(-1)).toMatchObject({ kind: "text", text: "Hi there" });
    expect(notifications).toBe(2); // the chunks, the stored event
    expect(frames).toHaveLength(0); // the flush that was asked for is cancelled

    // The next turn's chunks belong to the next text.
    transport.emit(chunk(LAST + 1, ORCHESTRATOR, "Next"));
    transport.emit(prompted(LAST + 2, ORCHESTRATOR));
    transport.emit(chunk(LAST + 2, ORCHESTRATOR, " turn"));
    frame();
    expect(text()).toBe("Next turn");
  });

  it("are shown before any other change of the view, too", async () => {
    transport.emit(chunk(LAST, ORCHESTRATOR, "partial"));
    // The status change drops the streamed text of the dead connection: it must drop this as well.
    transport.setStatus({ state: "closed", reason: "gone", retryInMs: 100 });
    expect(text()).toBeUndefined();
    expect(frames).toHaveLength(0);
  });

  it("older than what is applied are dropped when they arrive, as before", () => {
    transport.emit(agentText(LAST + 1, ORCHESTRATOR, "done"));
    transport.emit(chunk(LAST, ORCHESTRATOR, "stale")); // from before the stored text
    expect(frames).toHaveLength(0);
    expect(text()).toBeUndefined();
  });

  it("from after a stored event not seen yet start the catch-up at once, not at the frame", async () => {
    const calls = transport.callsOf("list_events").length;
    core.log = [...LOG, agentText(LAST + 1, ORCHESTRATOR, "late")].map((e, i) => ({ ...e, seq: i + 1 }));
    transport.emit(chunk(LAST + 1, ORCHESTRATOR, "late"));
    expect(transport.callsOf("list_events")).toHaveLength(calls + 1);
    await vi.waitFor(() => expect(project().cursor).toBe(LAST + 1));
  });

  it("do not reach the project that replaced the one they were for", async () => {
    transport.emit(chunk(LAST, ORCHESTRATOR, "for the old view"));
    core.snapshot = { ...FULL_RUN.start, seq: 0, sessions: [] };
    core.log = [];
    await store.openProject(PROJECT.path); // loads the project again: a new view
    await vi.waitFor(() => expect(project().phase).toBe("ready"));
    store.flushLive();
    expect(text()).toBeUndefined();
  });

  it("are shown by the timer too when no frame comes", async () => {
    vi.useFakeTimers();
    try {
      const other = new AppStore(transport); // the default scheduler: a frame, or LIVE_FLUSH_MAX_MS
      other.start();
      core.log = LOG;
      await other.openProject(PROJECT.path);
      await vi.waitFor(() => expect(other.getState().project?.phase).toBe("ready"));
      transport.emit(chunk(LAST, ORCHESTRATOR, "slow"));
      expect(other.getState().live.streaming[ORCHESTRATOR]).toBeUndefined();
      await vi.advanceTimersByTimeAsync(LIVE_FLUSH_MAX_MS);
      expect(other.getState().live.streaming[ORCHESTRATOR]?.message).toBe("slow");
      other.stop();
    } finally {
      vi.useRealTimers();
    }
  });

  it("not on screen (another project) are not collected", () => {
    transport.emit(ev(0, chunk(0, ORCHESTRATOR, "x").body, true, "beta"));
    expect(frames).toHaveLength(0);
  });
});
