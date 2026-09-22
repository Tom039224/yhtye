import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ApiEvent } from "../api/generated";
import { CommandError } from "../api/transport";
import { agentText, chunk, prompted } from "../test/events";
import { durable, FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { AppStore } from "./app";
import { ORCHESTRATOR } from "./transcript";

const LOG = durable(FULL_RUN);
const LAST = LOG[LOG.length - 1].seq;

let transport: MemoryTransport;
let core: FakeCore;
let store: AppStore;

beforeEach(() => {
  transport = new MemoryTransport();
  core = new FakeCore(PROJECT, FULL_RUN.start);
  core.attach(transport);
  store = new AppStore(transport);
  store.start();
});

function view() {
  const v = store.getState().project;
  if (!v) throw new Error("no project");
  return v;
}

/** Every durable seq that produced a transcript item, per session (to spot duplicates). */
function itemSeqs(): number[] {
  return Object.values(view().transcripts)
    .flat()
    .map((i) => i.seq);
}

async function openAndSettle() {
  await store.openProject(PROJECT.path);
  await vi.waitFor(() => expect(view().phase).toBe("ready"));
}

describe("AppStore sync", () => {
  it("loads snapshot + history in pages and ends at the core's state", async () => {
    core.log = LOG;
    core.pageCap = 7;
    await openAndSettle();
    expect(view().cursor).toBe(LAST);
    expect(view().state).toEqual(FULL_RUN.end.state);
    expect(view().sessions).toEqual(FULL_RUN.end.sessions);
    const pages = transport.callsOf("list_events");
    expect(pages.map((c) => c.after_seq)).toEqual([0, 7, 14, 21, 28, 35, 42, 49]);
  });

  it("buffers events pushed during the load and applies each once", async () => {
    const half = LOG.filter((e) => e.seq <= 30);
    core.log = half;
    const next = LOG.find((e) => e.seq === 31) as ApiEvent;
    core.before = (cmd) => {
      if (cmd.type !== "get_snapshot") return;
      // Pushed while the client is loading: duplicates of stored ones and a new one.
      for (const e of FULL_RUN.events.filter((x) => x.seq >= 28 && x.seq <= 30)) transport.emit(e);
      core.log = [...half, next];
      transport.emit(next);
    };
    await openAndSettle();
    expect(view().cursor).toBe(31);
    const seqs = itemSeqs();
    expect(new Set(seqs).size).toBe(seqs.length);
  });

  it("catches up with ListEvents when a durable event is missing", async () => {
    core.log = LOG.filter((e) => e.seq <= 20);
    await openAndSettle();
    core.log = LOG;
    transport.emit(LOG[LOG.length - 1]); // seq LAST: 21..LAST-1 were not pushed
    await vi.waitFor(() => expect(view().cursor).toBe(LAST));
    expect(transport.callsOf("list_events").at(-1)?.after_seq).toBe(20);
    expect(view().state).toEqual(FULL_RUN.end.state);
    // A late duplicate changes nothing.
    const before = view();
    transport.emit(LOG[10]);
    expect(view()).toBe(before);
  });

  it("treats a live event from after an unseen durable event as a gap", async () => {
    core.log = LOG.filter((e) => e.seq <= 5);
    await openAndSettle();
    core.log = LOG.filter((e) => e.seq <= 7);
    transport.emit(chunk(7, ORCHESTRATOR, "late"));
    await vi.waitFor(() => expect(view().cursor).toBe(7));
  });

  it("streams text and drops stale chunks replayed after a catch-up", async () => {
    core.log = [];
    core.snapshot = { ...FULL_RUN.start, seq: 0, sessions: [] };
    await openAndSettle();
    transport.emit(chunk(0, ORCHESTRATOR, "Hi "));
    transport.emit(chunk(0, ORCHESTRATOR, "there"));
    expect(view().streaming[ORCHESTRATOR].message).toBe("Hi there");
    transport.emit(agentText(1, ORCHESTRATOR, "Hi there"));
    expect(view().streaming[ORCHESTRATOR].message).toBe("");
    transport.emit(chunk(0, ORCHESTRATOR, "stale"));
    expect(view().streaming[ORCHESTRATOR].message).toBe("");
  });

  it("re-opens and catches up after a reconnect, dropping partial streams", async () => {
    core.log = LOG.filter((e) => e.seq <= 10);
    await openAndSettle();
    transport.emit(chunk(10, ORCHESTRATOR, "partial"));
    expect(view().streaming[ORCHESTRATOR]?.message).toBe("partial");
    transport.setStatus({ state: "closed", reason: "bridge restarted", retryInMs: 500 });
    expect(store.getState().connection).toMatchObject({ state: "closed", reason: "bridge restarted" });
    expect(view().streaming[ORCHESTRATOR]).toBeUndefined();
    core.log = LOG;
    const opens = transport.callsOf("open_project").length;
    transport.setStatus({ state: "open" });
    await vi.waitFor(() => expect(view().cursor).toBe(LAST));
    expect(transport.callsOf("open_project").length).toBe(opens + 1);
    expect(transport.callsOf("list_events").at(-1)?.after_seq).toBe(10);
    const seqs = itemSeqs();
    expect(new Set(seqs).size).toBe(seqs.length);
  });

  it("shows load failures and command errors", async () => {
    core.failures.set("get_snapshot", new CommandError("not_found", "project repo is not open"));
    await store.openProject(PROJECT.path);
    expect(view().phase).toBe("error");
    expect(store.getState().errors.at(-1)?.message).toContain("project repo is not open");

    core.failures.clear();
    core.log = LOG;
    await openAndSettle();
    core.failures.set("send_user_message", new CommandError("unavailable", "the project's orchestration has stopped"));
    expect(await store.sendMessage("hello")).toBe(false);
    expect(store.getState().errors.at(-1)?.message).toContain("orchestration has stopped");
    store.dismissError(store.getState().errors[0].id);
    expect(store.getState().errors.length).toBeLessThan(2);
  });

  it("sends user commands for the open project", async () => {
    core.log = LOG;
    await openAndSettle();
    expect(await store.sendMessage("do it")).toBe(true);
    await store.cancelTurn();
    await store.cancelTask("T-1");
    await store.cancelGroup("G-1");
    expect(transport.calls.slice(-4)).toEqual([
      { type: "send_user_message", project: "repo", text: "do it" },
      { type: "cancel_orchestrator_turn", project: "repo" },
      { type: "cancel_task", project: "repo", task: "T-1" },
      { type: "cancel_group", project: "repo", group: "G-1" },
    ]);
  });

  it("ignores events of other projects and tracks the orchestrator's turn", async () => {
    core.log = LOG;
    await openAndSettle();
    const before = view();
    transport.emit({ ...prompted(LAST + 1, ORCHESTRATOR), project: "other" });
    expect(view()).toBe(before);
    transport.emit(prompted(LAST + 1, ORCHESTRATOR));
    expect(view().sessions.find((s) => s.session_key === ORCHESTRATOR)?.turn_running).toBe(true);
  });
});
