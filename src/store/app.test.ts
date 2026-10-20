import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ApiEvent } from "../api/generated";
import { CommandError } from "../api/transport";
import { agentText, chunk, prompted, turnEnded } from "../test/events";
import { durable, FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { ACTIVITY_REFRESH_MS, AppStore } from "./app";
import { ORCHESTRATOR } from "../test/events";

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
    await store.retryGroupMerge("G-2");
    expect(transport.calls.slice(-5)).toEqual([
      { type: "send_user_message", project: "repo", chat: "C-1", text: "do it" },
      { type: "cancel_orchestrator_turn", project: "repo", chat: "C-1" },
      { type: "cancel_task", project: "repo", task: "T-1" },
      { type: "cancel_group", project: "repo", group: "G-1" },
      { type: "retry_group_merge", project: "repo", group: "G-2" },
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

describe("AppStore lazy history", () => {
  async function openWindowed() {
    store.stop();
    store = new AppStore(transport, { historyWindow: 10, historyPage: 7 });
    store.start();
    core.log = LOG;
    core.snapshot = FULL_RUN.end;
    await openAndSettle();
    expect(view().historyStart).toBe(LAST - 9);
  }

  const olderCalls = () => transport.callsOf("list_events").filter((c) => c.after_seq < LAST - 10);

  it("reads one page for overlapping requests", async () => {
    await openWindowed();
    await Promise.all([store.loadOlderHistory(), store.loadOlderHistory(), store.loadOlderHistory()]);
    expect(view().historyStart).toBe(LAST - 16);
    expect(olderCalls()).toHaveLength(1);
    expect(view().loadingOlder).toBe(false);
  });

  it("does nothing once the whole log is loaded", async () => {
    await openWindowed();
    while (view().historyStart > 1) await store.loadOlderHistory();
    const calls = transport.callsOf("list_events").length;
    await store.loadOlderHistory();
    expect(transport.callsOf("list_events")).toHaveLength(calls);
  });

  it("keeps a failure for the history row, and a retry clears it", async () => {
    await openWindowed();
    core.failures.set("list_events", new CommandError("transport", "bridge restarted"));
    await store.loadOlderHistory();
    expect(view().olderError).toBe("bridge restarted");
    expect(view().loadingOlder).toBe(false);
    expect(view().historyStart).toBe(LAST - 9);

    core.failures.delete("list_events");
    const retry = store.loadOlderHistory();
    expect(view().olderError).toBeNull();
    await retry;
    expect(view().olderError).toBeNull();
    expect(view().historyStart).toBe(LAST - 16);
  });
});

describe("AppStore background activity", () => {
  const BETA = { id: "beta", name: "beta", path: "/tmp/beta", open: true, running: false, awake: false };
  const beta = (seq: number, ev: ApiEvent): ApiEvent => ({ ...ev, seq, project: "beta" });
  const running = (id: string) => store.getState().projects.find((p) => p.id === id)?.running;
  const listCalls = () => transport.callsOf("list_projects").length;

  beforeEach(() => {
    core.others = [BETA];
  });

  it("re-reads the project list when a turn starts or ends in a project that is not on screen", async () => {
    core.log = LOG;
    await openAndSettle();
    await store.refreshProjects();
    expect(running("beta")).toBe(false);
    const before = view();
    const listed = listCalls();

    core.others = [{ ...BETA, running: true }];
    transport.emit(beta(1, prompted(1, ORCHESTRATOR)));
    expect(listCalls()).toBe(listed); // not at once: changes come in bursts
    await vi.waitFor(() => expect(running("beta")).toBe(true));
    expect(listCalls()).toBe(listed + 1);

    core.others = [{ ...BETA, running: false }];
    transport.emit(beta(2, turnEnded(2, ORCHESTRATOR)));
    await vi.waitFor(() => expect(running("beta")).toBe(false));
    expect(view()).toBe(before); // the project on screen is untouched
  });

  it("asks once for a burst of changes, and not at all for output or for the project on screen", async () => {
    core.log = LOG;
    await openAndSettle();
    const listed = listCalls();

    transport.emit(beta(1, agentText(1, ORCHESTRATOR, "hello")));
    transport.emit(beta(2, chunk(2, ORCHESTRATOR, "hel")));
    transport.emit(prompted(LAST + 1, ORCHESTRATOR)); // repo is on screen
    await new Promise((r) => setTimeout(r, ACTIVITY_REFRESH_MS * 2));
    expect(listCalls()).toBe(listed);

    transport.emit(beta(3, prompted(3, ORCHESTRATOR)));
    transport.emit(beta(4, turnEnded(4, ORCHESTRATOR)));
    transport.emit(beta(5, prompted(5, ORCHESTRATOR)));
    await vi.waitFor(() => expect(listCalls()).toBe(listed + 1));
    await new Promise((r) => setTimeout(r, ACTIVITY_REFRESH_MS * 2));
    expect(listCalls()).toBe(listed + 1);
  });

  it("also follows the projects the core opened itself while none is on screen", async () => {
    await store.refreshProjects();
    expect(store.getState().project).toBeNull();
    core.others = [{ ...BETA, running: true }];
    transport.emit(beta(1, prompted(1, ORCHESTRATOR)));
    await vi.waitFor(() => expect(running("beta")).toBe(true));
  });

  it("keeps the flag of the project it leaves as its view showed it, and follows it afterwards", async () => {
    core.snapshot = { ...FULL_RUN.start, sessions: [{ ...FULL_RUN.end.sessions[0], status: "live", turn_running: true }] };
    core.info = { ...PROJECT, running: true }; // what the core says of it too
    await openAndSettle();
    await store.refreshProjects();
    expect(running("repo")).toBe(true);
    expect(running("beta")).toBe(false);

    // Leaving it: its ring goes on turning (no moment of reading idle) ...
    core.snapshot = { ...FULL_RUN.start, sessions: [] };
    core.info = { ...PROJECT, running: false };
    let during: boolean | undefined;
    core.before = (cmd) => {
      if (cmd.type === "get_snapshot") during = running("repo");
    };
    await store.openProject(BETA.path);
    expect(during).toBe(true);

    // ... until the core says it has stopped.
    await vi.waitFor(() => expect(running("repo")).toBe(false));
  });
});
