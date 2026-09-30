// The store's chat handling (Stage 8b): selection and its memory, sending to
// the selected chat, creating chats and branches, per-chat transcripts.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CommandError } from "../api/transport";
import { CHAT_INFOS, CHAT_LOG, chatSnapshot, MAIN_WT } from "../test/chats";
import { ev, prompted, userMessage } from "../test/events";
import { PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { AppStore, chatPrefKey, PING_INTERVAL_MS, PING_TIMEOUT_MS } from "./app";
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

beforeEach(() => {
  transport = new MemoryTransport();
  core = new FakeCore(PROJECT, chatSnapshot());
  core.log = CHAT_LOG;
  core.attach(transport);
  prefs = memoryPrefs();
});

describe("chat selection", () => {
  it("opens the most recently used chat", async () => {
    const s = newStore();
    await open(s);
    expect(view(s).selectedChat).toBe("C-2");
  });

  it("remembers the selection per project and restores it when the project is opened again", async () => {
    const s = newStore();
    await open(s);
    s.selectChat("C-1");
    expect(prefs.get(chatPrefKey(PROJECT.id))).toBe("C-1");
    const again = newStore();
    await open(again);
    expect(view(again).selectedChat).toBe("C-1");
    // An unknown remembered chat falls back to the last used one.
    prefs.set(chatPrefKey(PROJECT.id), "C-9");
    const third = newStore();
    await open(third);
    expect(view(third).selectedChat).toBe("C-2");
  });

  it("ignores a chat the project does not have", async () => {
    const s = newStore();
    await open(s);
    s.selectChat("C-9");
    expect(view(s).selectedChat).toBe("C-2");
  });

  it("has no selected chat in a project without chats", async () => {
    core.snapshot = chatSnapshot([], { ...core.snapshot.state, chats: [], groups: [], tasks: [] });
    core.log = [];
    const s = newStore();
    await open(s);
    expect(view(s).selectedChat).toBeNull();
    expect(await s.sendMessage("hi")).toBe(false);
    await s.cancelTurn();
    expect(transport.callsOf("send_user_message")).toHaveLength(0);
    // No chat is made up for the user (Stage 8a's placeholder is gone).
    expect(transport.callsOf("create_chat")).toHaveLength(0);
  });
});

describe("transcripts per chat", () => {
  it("keeps each chat's conversation under its own orchestrator session", async () => {
    const s = newStore();
    await open(s);
    const t = view(s).transcripts;
    expect(t[orchestratorKey("C-1")].map((i) => i.kind)).toEqual(["user", "text"]);
    expect(t[orchestratorKey("C-2")].map((i) => i.kind)).toEqual(["user", "text"]);
    expect(t[orchestratorKey("C-1")][0]).toMatchObject({ text: "Refactor checkout" });
    expect(t[orchestratorKey("C-2")][0]).toMatchObject({ text: "Add feature x" });
  });

  it("puts a live message into the chat it is for and marks a notification for another chat unread", async () => {
    const s = newStore();
    await open(s);
    transport.emit(userMessage(5, 3, "more", "C-1"));
    const notice = ev(6, {
      type: "domain",
      event: { type: "inbox_queued", entry: { id: 4, chat: "C-1", item: { kind: "help_raised", attrs: [], body: "stuck" } } },
    });
    transport.emit(notice);
    const t = view(s).transcripts;
    expect(t[orchestratorKey("C-1")].map((i) => i.kind)).toEqual(["user", "text", "user", "notice"]);
    expect(t[orchestratorKey("C-2")]).toHaveLength(2);
    expect(view(s).unread).toEqual({ "C-1": true });
    s.selectChat("C-1");
    expect(view(s).unread).toEqual({});
  });

  it("stamps the last use of a chat when its orchestrator is prompted", async () => {
    const s = newStore();
    await open(s);
    const event = prompted(5, orchestratorKey("C-1"));
    transport.emit(event);
    expect(view(s).chats.find((c) => c.id === "C-1")?.last_used_ms).toBe(event.ts_ms);
    expect(view(s).chats.find((c) => c.id === "C-2")?.last_used_ms).toBe(CHAT_INFOS[0].last_used_ms);
  });
});

describe("commands", () => {
  it("sends and cancels on the selected chat", async () => {
    const s = newStore();
    await open(s);
    expect(await s.sendMessage("one")).toBe(true);
    s.selectChat("C-1");
    expect(await s.sendMessage("two")).toBe(true);
    await s.cancelTurn();
    expect(transport.calls.slice(-3)).toEqual([
      { type: "send_user_message", project: "repo", chat: "C-2", text: "one" },
      { type: "send_user_message", project: "repo", chat: "C-1", text: "two" },
      { type: "cancel_orchestrator_turn", project: "repo", chat: "C-1" },
    ]);
  });

  it("creates a chat in a worktree, or in a branch's, and selects it", async () => {
    const s = newStore();
    await open(s);
    expect(await s.createChat({ worktree: MAIN_WT })).toBe(true);
    expect(view(s).selectedChat).toBe("C-3");
    expect(view(s).chats.find((c) => c.id === "C-3")?.worktree).toBe(MAIN_WT);
    expect(await s.createChat({ branch: "feat/x" })).toBe(true);
    expect(transport.callsOf("create_chat")).toEqual([
      { type: "create_chat", project: "repo", worktree: MAIN_WT },
      { type: "create_chat", project: "repo", branch: "feat/x" },
    ]);
  });

  it("re-reads git before selecting a chat made in a branch's new worktree", async () => {
    const s = newStore();
    await open(s);
    // Whether git listed the chat's worktree when the chat first became selected.
    let listedWhenSelected: boolean | null = null;
    s.subscribe(() => {
      if (listedWhenSelected !== null || s.getState().project?.selectedChat !== "C-3") return;
      const worktrees = s.getState().git?.overview?.worktrees ?? [];
      listedWhenSelected = worktrees.some((w) => w.branch === "feat/x");
    });
    expect(await s.createChat({ branch: "feat/x" })).toBe(true);
    // No "(見つからない作業ツリー)" flash.
    expect(listedWhenSelected).toBe(true);
  });

  it("shows the error and keeps the selection when a chat cannot be created", async () => {
    const s = newStore();
    await open(s);
    core.failures.set("create_chat", new CommandError("not_found", "branch gone: feat/x"));
    expect(await s.createChat({ branch: "feat/x" })).toBe(false);
    expect(s.getState().errors[0].message).toContain("branch gone");
    expect(view(s).selectedChat).toBe("C-2");
  });

  it("creates a branch from a start point, opens its first chat and re-reads git", async () => {
    const s = newStore();
    await open(s);
    await s.createBranch("feat/new", "main");
    expect(transport.callsOf("create_branch")).toEqual([
      { type: "create_branch", project: "repo", name: "feat/new", from: "main" },
    ]);
    expect(view(s).selectedChat).toBe("C-3");
    // Git is already re-read when the chat is selected: no "deleted branch" flash.
    expect(s.getState().git?.overview?.branches.map((b) => b.name)).toContain("feat/new");
  });

  it("rejects with the core's error so the form can show it", async () => {
    const s = newStore();
    await open(s);
    core.branchError = new CommandError("conflict", "branch feat/x already exists");
    await expect(s.createBranch("feat/x")).rejects.toMatchObject({ message: "branch feat/x already exists" });
    expect(view(s).selectedChat).toBe("C-2");
  });
});

describe("renaming and deleting chats", () => {
  it("renames a chat through the core and shows the title at once", async () => {
    const s = newStore();
    await open(s);
    await s.renameChat("C-1", "  Checkout\nwork ");
    expect(transport.callsOf("rename_chat")).toEqual([
      { type: "rename_chat", project: "repo", chat: "C-1", title: "  Checkout\nwork " },
    ]);
    expect(view(s).chats.find((c) => c.id === "C-1")?.title).toBe("Checkout work");
    expect(view(s).chats.find((c) => c.id === "C-2")?.title).toBe("Add feature x");
    // The core's event says the same and changes nothing more.
    transport.emit(ev(5, { type: "domain", event: { type: "chat_titled", chat: "C-1", title: "Checkout work" } }));
    expect(view(s).chats.find((c) => c.id === "C-1")?.title).toBe("Checkout work");
    expect(view(s).state?.chats.find((c) => c.id === "C-1")?.title).toBe("Checkout work");
  });

  it("rejects with the core's error so the editor can show it, and shows no app error", async () => {
    const s = newStore();
    await open(s);
    core.failures.set("rename_chat", new CommandError("invalid_argument", "the title must not be empty"));
    await expect(s.renameChat("C-1", " ")).rejects.toMatchObject({ code: "invalid_argument" });
    expect(view(s).chats.find((c) => c.id === "C-1")?.title).toBe("Refactor checkout");
    expect(s.getState().errors).toEqual([]);
  });

  it("deletes a chat: it leaves the list and another chat is shown if it was on screen", async () => {
    const s = newStore();
    await open(s);
    expect(view(s).selectedChat).toBe("C-2");
    await s.deleteChat("C-2");
    expect(transport.callsOf("delete_chat")).toEqual([{ type: "delete_chat", project: "repo", chat: "C-2" }]);
    expect(view(s).chats.map((c) => c.id)).toEqual(["C-1"]);
    expect(view(s).selectedChat).toBe("C-1");
    // The event that follows finds nothing left to do.
    transport.emit(ev(5, { type: "domain", event: { type: "chat_deleted", chat: "C-2" } }));
    expect(view(s).chats.map((c) => c.id)).toEqual(["C-1"]);
    expect(view(s).state?.groups.map((g) => g.id)).toEqual(["G-1"]);
    expect(view(s).transcripts[orchestratorKey("C-2")]).toBeUndefined();
  });

  it("keeps the chat and rejects with the refusal when the core will not delete it", async () => {
    const s = newStore();
    await open(s);
    core.failures.set("delete_chat", new CommandError("invalid_state", "chat C-2 still has group G-2"));
    await expect(s.deleteChat("C-2")).rejects.toMatchObject({ code: "invalid_state", message: "chat C-2 still has group G-2" });
    expect(view(s).chats.map((c) => c.id)).toEqual(["C-2", "C-1"]);
    expect(view(s).selectedChat).toBe("C-2");
  });
});

describe("pinging the core", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("pings when the connection opens and keeps the host name and the time of the answer", async () => {
    const s = newStore();
    expect(s.getState().host).toEqual({ name: null, lastPingAt: null });
    await vi.advanceTimersByTimeAsync(0);
    expect(transport.callsOf("ping")).toHaveLength(1);
    expect(s.getState().host).toEqual({ name: "test-host", lastPingAt: Date.now() });
    s.stop();
  });

  it("pings every few seconds, and stops while the connection is closed", async () => {
    const s = newStore();
    await vi.advanceTimersByTimeAsync(PING_INTERVAL_MS * 2);
    expect(transport.callsOf("ping")).toHaveLength(3);
    const answered = s.getState().host.lastPingAt;
    transport.setStatus({ state: "closed", reason: "gone", retryInMs: null });
    await vi.advanceTimersByTimeAsync(PING_INTERVAL_MS * 3);
    expect(transport.callsOf("ping")).toHaveLength(3);
    expect(s.getState().host).toEqual({ name: "test-host", lastPingAt: answered });
    // It picks up again on reconnecting.
    transport.setStatus({ state: "open" });
    await vi.advanceTimersByTimeAsync(0);
    expect(transport.callsOf("ping")).toHaveLength(4);
    s.stop();
  });

  it("counts a failed ping as missed without showing an error, and keeps the last answer", async () => {
    const s = newStore();
    await vi.advanceTimersByTimeAsync(0);
    const answered = s.getState().host.lastPingAt;
    core.failures.set("ping", new CommandError("internal", "boom"));
    await vi.advanceTimersByTimeAsync(PING_INTERVAL_MS * 2);
    expect(s.getState().host.lastPingAt).toBe(answered);
    expect(s.getState().errors).toEqual([]);
    s.stop();
  });

  it("gives up on a ping nobody answers and asks again", async () => {
    core.before = (cmd) => (cmd.type === "ping" ? new Promise<void>(() => {}) : undefined);
    const s = newStore();
    await vi.advanceTimersByTimeAsync(PING_TIMEOUT_MS + PING_INTERVAL_MS);
    expect(transport.callsOf("ping").length).toBeGreaterThanOrEqual(2);
    expect(s.getState().host.lastPingAt).toBeNull();
    s.stop();
  });
});
