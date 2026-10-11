// The store with other clients on the same core (T-65): app-wide events
// (projects_changed, settings_changed, resync), a chat being started by another
// client, the chat on screen deleted elsewhere, and a chat made elsewhere in a
// worktree git was not read with yet.

import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ApiEventBody } from "../api/generated";
import { CHAT_LOG, chatSnapshot } from "../test/chats";
import { ev } from "../test/events";
import { PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { placeOf } from "../ui/branchTree";
import { AppStore, chatPrefKey, type SettingsChange } from "./app";
import { orchestratorKey } from "./chats";
import { memoryPrefs, type Prefs } from "./prefs";

let transport: MemoryTransport;
let core: FakeCore;
let prefs: Prefs;

function newStore(): AppStore {
  const store = new AppStore(transport, { prefs });
  store.start();
  return store;
}

const view = (s: AppStore) => {
  const v = s.getState().project;
  if (!v) throw new Error("no project");
  return v;
};

async function open(s: AppStore) {
  await s.openProject(PROJECT.path);
  await vi.waitFor(() => expect(view(s).phase).toBe("ready"));
}

/** An app-wide event, as the core sends it (no project, seq 0, live). */
function appWide(body: ApiEventBody) {
  return ev(0, body, true, "");
}

beforeEach(() => {
  transport = new MemoryTransport();
  core = new FakeCore(PROJECT, chatSnapshot());
  core.log = CHAT_LOG;
  core.attach(transport);
  prefs = memoryPrefs();
});

describe("app-wide events", () => {
  it("re-reads the project list when another client opens a project", async () => {
    const s = newStore();
    await open(s);
    const other = { id: "other", name: "other", path: "/tmp/other", open: true, running: false };
    core.others = [other];
    const before = transport.callsOf("list_projects").length;
    transport.emit(appWide({ type: "projects_changed", project: "other" }));
    await vi.waitFor(() => expect(s.getState().projects.map((p) => p.id)).toEqual([PROJECT.id, "other"]));
    expect(transport.callsOf("list_projects").length).toBe(before + 1);
    // Not folded into the open project.
    expect(view(s).cursor).toBe(CHAT_LOG.length);
  });

  it("passes settings changes to the settings screens", async () => {
    const s = newStore();
    await open(s);
    const seen: SettingsChange[] = [];
    const off = s.onSettingsChanged((c) => seen.push(c));
    transport.emit(appWide({ type: "settings_changed", kind: "agent_settings", project: "repo", revision: 7 }));
    transport.emit(appWide({ type: "settings_changed", kind: "secrets" }));
    off();
    transport.emit(appWide({ type: "settings_changed", kind: "harnesses" }));
    expect(seen).toEqual([
      { kind: "agent_settings", project: "repo", revision: 7 },
      { kind: "secrets", project: null, revision: null },
    ]);
  });

  it("catches up after a resync: the log, the project list, git, the chat on screen and the settings", async () => {
    const s = newStore();
    await open(s);
    const seen: SettingsChange[] = [];
    s.onSettingsChanged((c) => seen.push(c));
    // Events this client never got (they were dropped for it).
    const missed = ev(CHAT_LOG.length + 1, { type: "prompted", session: orchestratorKey("C-2"), text: "missed" });
    core.log = [...CHAT_LOG, missed];
    transport.calls.length = 0;
    transport.emit(appWide({ type: "resync" }));
    await vi.waitFor(() => expect(view(s).cursor).toBe(CHAT_LOG.length + 1));
    expect(transport.callsOf("list_events")[0].after_seq).toBe(CHAT_LOG.length);
    expect(transport.callsOf("list_projects")).toHaveLength(1);
    expect(transport.callsOf("get_git_overview")).toHaveLength(1);
    expect(transport.callsOf("open_chat")).toEqual([{ type: "open_chat", project: "repo", chat: "C-2" }]);
    expect(seen).toEqual([{ kind: null, project: null, revision: null }]);
  });

  it("tells the settings screens that anything may have changed after a reconnect", async () => {
    const s = newStore();
    await open(s);
    const seen: SettingsChange[] = [];
    s.onSettingsChanged((c) => seen.push(c));
    transport.setStatus({ state: "closed", reason: "gone", retryInMs: 100 });
    transport.setStatus({ state: "open" });
    expect(seen).toEqual([{ kind: null, project: null, revision: null }]);
  });
});

describe("a chat starting", () => {
  it("shows a chat another client is starting, from its live event", async () => {
    const s = newStore();
    await open(s);
    expect(view(s).resuming).toEqual({});
    transport.emit(ev(CHAT_LOG.length, { type: "session_starting", session: orchestratorKey("C-1") }, true));
    expect(view(s).resuming).toEqual({ "C-1": true });
    // Its start ends it.
    const started = ev(CHAT_LOG.length + 1, {
      type: "session_started",
      session: orchestratorKey("C-1"),
      role: "orchestrator",
      task: null,
      pid: 1,
      acp_session_id: "a",
      resumed: false,
      agent: null,
    });
    core.log = [...CHAT_LOG, started];
    transport.emit(started);
    expect(view(s).resuming).toEqual({});
  });

  it("shows the chats being started when the project is loaded (a client that connects later)", async () => {
    core.snapshot = { ...chatSnapshot(), starting: [orchestratorKey("C-1"), "T-1/0"] };
    const s = newStore();
    await open(s);
    expect(view(s).resuming).toEqual({ "C-1": true });
  });
});

describe("the chat on screen deleted by another client", () => {
  it("switches to another chat, remembers it, opens it and drops the deleted chat's draft", async () => {
    const s = newStore();
    await open(s);
    expect(view(s).selectedChat).toBe("C-2");
    s.drafts.set(PROJECT.id, "C-2", "half-written");
    transport.calls.length = 0;
    const deleted = ev(CHAT_LOG.length + 1, { type: "domain", event: { type: "chat_deleted", chat: "C-2" } });
    core.log = [...CHAT_LOG, deleted];
    transport.emit(deleted);
    expect(view(s).selectedChat).toBe("C-1");
    expect(view(s).chats.map((c) => c.id)).toEqual(["C-1"]);
    expect(prefs.get(chatPrefKey(PROJECT.id))).toBe("C-1");
    expect(s.drafts.get(PROJECT.id, "C-2")).toBe("");
    expect(transport.callsOf("open_chat")).toEqual([{ type: "open_chat", project: "repo", chat: "C-1" }]);
  });

  it("leaves the selection alone when another chat is deleted", async () => {
    const s = newStore();
    await open(s);
    s.drafts.set(PROJECT.id, "C-1", "draft");
    transport.calls.length = 0;
    const deleted = ev(CHAT_LOG.length + 1, { type: "domain", event: { type: "chat_deleted", chat: "C-1" } });
    core.log = [...CHAT_LOG, deleted];
    transport.emit(deleted);
    expect(view(s).selectedChat).toBe("C-2");
    expect(s.drafts.get(PROJECT.id, "C-1")).toBe("");
    expect(transport.callsOf("open_chat")).toEqual([]);
  });
});

describe("a chat made by another client", () => {
  it("is shown, remembered and opened when the project had none", async () => {
    core.snapshot = { ...chatSnapshot([], { ...chatSnapshot().state, chats: [], groups: [], tasks: [] }), seq: 0 };
    core.log = [];
    const s = newStore();
    await open(s);
    expect(view(s).selectedChat).toBeNull();
    const chat = { id: "C-1", worktree: "/repo", title: null };
    const created = ev(1, { type: "domain", event: { type: "chat_created", chat } });
    core.log = [created];
    core.snapshot = { ...core.snapshot, chats: [{ ...chat, created_ms: 1, last_used_ms: 1 }] };
    transport.emit(created);
    expect(view(s).selectedChat).toBe("C-1");
    expect(prefs.get(chatPrefKey(PROJECT.id))).toBe("C-1");
    expect(transport.callsOf("open_chat")).toEqual([{ type: "open_chat", project: "repo", chat: "C-1" }]);
    // A second one does not take the screen.
    const second = ev(2, { type: "domain", event: { type: "chat_created", chat: { ...chat, id: "C-2" } } });
    transport.emit(second);
    expect(view(s).selectedChat).toBe("C-1");
  });

  it("reads git at once and shows the chat's place as unknown, not missing, until then", async () => {
    {
      const s = newStore();
      await s.openProject(PROJECT.path);
      await vi.waitFor(() => expect(view(s).phase).toBe("ready"));
      await vi.waitFor(() => expect(s.getState().git?.loading).toBe(false));
      const wt = "/data/worktrees/repo/branches/new";
      const chat = { id: "C-3", worktree: wt, title: null };
      // The answer to the git read is held back until the test lets it go.
      let release: () => void = () => {};
      const held = new Promise<void>((r) => {
        release = r;
      });
      core.before = async (cmd) => {
        if (cmd.type === "get_git_overview") await held;
      };
      const reads = transport.callsOf("get_git_overview").length;
      const created = ev(CHAT_LOG.length + 1, { type: "domain", event: { type: "chat_created", chat } });
      core.log = [...CHAT_LOG, created];
      transport.emit(created);
      // Read at once, not after the debounce delay.
      expect(transport.callsOf("get_git_overview").length).toBe(reads + 1);
      const git = () => s.getState().git;
      const info = { ...chat, created_ms: 1, last_used_ms: 1 };
      expect(git()?.awaiting).toEqual(["C-3"]);
      expect(placeOf(info, git()?.overview ?? null, git()?.awaiting.includes("C-3"))).toEqual({ kind: "unknown" });
      // Git answers with the new worktree.
      core.git = { ...core.git, worktrees: [...core.git.worktrees, { path: wt, branch: "new", head_sha: null, is_main: false, missing: false }] };
      release();
      await vi.waitFor(() => expect(git()?.loading).toBe(false));
      expect(git()?.awaiting).toEqual([]);
      expect(placeOf(info, git()?.overview ?? null, false)).toEqual({ kind: "branch", name: "new" });
      // A chat whose worktree git does not list after reading is missing.
      expect(placeOf({ ...info, worktree: "/gone" }, git()?.overview ?? null, false)).toEqual({ kind: "missing" });
    }
  });
});
