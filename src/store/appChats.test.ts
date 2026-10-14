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

describe("opening a chat brings its orchestrator back", () => {
  const sessionStarted = (seq: number, chat: string) =>
    ev(seq, {
      type: "session_started",
      session: orchestratorKey(chat),
      role: "orchestrator",
      task: null,
      pid: 1,
      acp_session_id: "acp-1",
      resumed: true,
      agent: null,
    });

  it("asks the core to open the chat that is shown when the project loads", async () => {
    const s = newStore();
    await open(s);
    await vi.waitFor(() => expect(transport.callsOf("open_chat")).toHaveLength(1));
    expect(transport.callsOf("open_chat")[0]).toEqual({ type: "open_chat", project: "repo", chat: "C-2" });
  });

  it("asks again for each chat that is selected, but not for the one already shown", async () => {
    const s = newStore();
    await open(s);
    await vi.waitFor(() => expect(transport.callsOf("open_chat")).toHaveLength(1));
    s.selectChat("C-2");
    s.selectChat("C-1");
    await vi.waitFor(() => expect(transport.callsOf("open_chat")).toHaveLength(2));
    expect(transport.callsOf("open_chat")[1]).toEqual({ type: "open_chat", project: "repo", chat: "C-1" });
    // An unknown chat is not selected, so nothing is asked.
    s.selectChat("C-9");
    expect(transport.callsOf("open_chat")).toHaveLength(2);
  });

  it("shows the chat as starting until its session is up", async () => {
    core.starting.add("C-1");
    const s = newStore();
    await open(s);
    s.selectChat("C-1");
    await vi.waitFor(() => expect(view(s).resuming).toEqual({ "C-1": true }));
    transport.emit(sessionStarted(5, "C-1"));
    expect(view(s).resuming).toEqual({});
    expect(view(s).sessions.find((x) => x.session_key === orchestratorKey("C-1"))?.status).toBe("live");
  });

  it("stops showing the chat as starting when its start fails for good, not when a retry follows", async () => {
    core.starting.add("C-1");
    const s = newStore();
    await open(s);
    s.selectChat("C-1");
    await vi.waitFor(() => expect(view(s).resuming).toEqual({ "C-1": true }));
    const failed = (seq: number, retrying: boolean) =>
      ev(seq, { type: "session_failed", session: orchestratorKey("C-1"), error: "boom", retrying });
    transport.emit(failed(5, true));
    expect(view(s).resuming).toEqual({ "C-1": true });
    transport.emit(failed(6, false));
    expect(view(s).resuming).toEqual({});
  });

  it("is not starting when the core has nothing to start", async () => {
    const s = newStore();
    await open(s);
    s.selectChat("C-1");
    await vi.waitFor(() => expect(transport.callsOf("open_chat")).toHaveLength(2));
    expect(view(s).resuming).toEqual({});
  });

  it("shows an error when the core refuses to open the chat", async () => {
    core.failures.set("open_chat", new CommandError("not_found", "no chat C-2"));
    const s = newStore();
    await open(s);
    await vi.waitFor(() => expect(s.getState().errors.map((e) => e.message)).toEqual(["open_chat failed: no chat C-2"]));
    expect(view(s).resuming).toEqual({});
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
    expect(transport.calls.filter((c) => c.type !== "open_chat").slice(-3)).toEqual([
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

  it("reads git once for a branch it created, and not again for the chat's event that follows", async () => {
    const s = newStore();
    await open(s);
    await vi.waitFor(() => expect(s.getState().git?.loading).toBe(false));
    const reads = transport.callsOf("get_git_overview").length;
    await s.createBranch("feat/new", "main");
    expect(transport.callsOf("get_git_overview")).toHaveLength(reads + 1);
    const chat = view(s).chats.find((c) => c.id === "C-3");
    if (!chat) throw new Error("the new chat is not listed");
    transport.emit(ev(CHAT_LOG.length + 1, { type: "domain", event: { type: "chat_created", chat: { id: chat.id, worktree: chat.worktree, title: null } } }));
    expect(transport.callsOf("get_git_overview")).toHaveLength(reads + 1);
    expect(s.getState().git?.awaiting).toEqual([]);
  });

  it("reads git once for a branch it created when the chat's event comes before the answer", async () => {
    const s = newStore();
    await open(s);
    await vi.waitFor(() => expect(s.getState().git?.loading).toBe(false));
    const reads = transport.callsOf("get_git_overview").length;
    const worktree = "/data/worktrees/repo/branches/feat-new";
    core.before = (cmd) => {
      if (cmd.type !== "create_branch") return;
      core.git = { ...core.git, worktrees: [...core.git.worktrees, { path: worktree, branch: "feat/new", head_sha: null, is_main: false, missing: false }] };
      transport.emit(ev(CHAT_LOG.length + 1, { type: "domain", event: { type: "chat_created", chat: { id: "C-3", worktree, title: null } } }));
    };
    await s.createBranch("feat/new", "main");
    await vi.waitFor(() => expect(s.getState().git?.loading).toBe(false));
    expect(transport.callsOf("get_git_overview")).toHaveLength(reads + 1);
    expect(view(s).selectedChat).toBe("C-3");
    expect(s.getState().git?.awaiting).toEqual([]);
  });

  it("reads git once for a chat it created in a branch's new worktree, and not again for the event", async () => {
    const s = newStore();
    await open(s);
    await vi.waitFor(() => expect(s.getState().git?.loading).toBe(false));
    const reads = transport.callsOf("get_git_overview").length;
    expect(await s.createChat({ branch: "feat/y" })).toBe(true);
    expect(transport.callsOf("get_git_overview")).toHaveLength(reads + 1);
    const chat = view(s).chats.find((c) => c.id === "C-3");
    if (!chat) throw new Error("the new chat is not listed");
    transport.emit(ev(CHAT_LOG.length + 1, { type: "domain", event: { type: "chat_created", chat: { id: chat.id, worktree: chat.worktree, title: null } } }));
    expect(transport.callsOf("get_git_overview")).toHaveLength(reads + 1);
  });

  it("does not read git for a chat it created in an existing worktree", async () => {
    const s = newStore();
    await open(s);
    await vi.waitFor(() => expect(s.getState().git?.loading).toBe(false));
    const reads = transport.callsOf("get_git_overview").length;
    expect(await s.createChat({ worktree: MAIN_WT })).toBe(true);
    transport.emit(ev(CHAT_LOG.length + 1, { type: "domain", event: { type: "chat_created", chat: { id: "C-3", worktree: MAIN_WT, title: null } } }));
    expect(transport.callsOf("get_git_overview")).toHaveLength(reads);
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

  it("forgets a deleted chat's draft, and keeps it when the core refuses", async () => {
    const s = newStore();
    await open(s);
    s.drafts.set("repo", "C-2", "typed");
    s.drafts.set("repo", "C-1", "other");
    core.failures.set("delete_chat", new CommandError("invalid_state", "chat C-2 still has group G-2"));
    await expect(s.deleteChat("C-2")).rejects.toMatchObject({ code: "invalid_state" });
    expect(s.drafts.get("repo", "C-2")).toBe("typed");
    core.failures.delete("delete_chat");
    await s.deleteChat("C-2");
    expect(s.drafts.get("repo", "C-2")).toBe("");
    expect(s.drafts.get("repo", "C-1")).toBe("other");
  });

  it("keeps drafts when the project is opened again", async () => {
    const s = newStore();
    await open(s);
    s.drafts.set("repo", "C-1", "typed");
    await open(s);
    expect(s.drafts.get("repo", "C-1")).toBe("typed");
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
    expect(s.getState().host).toEqual({ name: null, headless: false, lastPingAt: null });
    await vi.advanceTimersByTimeAsync(0);
    expect(transport.callsOf("ping")).toHaveLength(1);
    expect(s.getState().host).toEqual({ name: "test-host", headless: false, lastPingAt: Date.now() });
    s.stop();
  });

  it("keeps that the host runs without a GUI, and reads a core that does not say as having one", async () => {
    core.headless = true;
    const s = newStore();
    await vi.advanceTimersByTimeAsync(0);
    expect(s.getState().host.headless).toBe(true);
    s.stop();

    // A core that predates the field leaves it out of its pong.
    const handle = core.handle.bind(core);
    core.handle = (cmd) => {
      const reply = handle(cmd);
      if (reply.type !== "pong") return reply;
      const { headless: _headless, ...older } = reply;
      return older as typeof reply;
    };
    const old = newStore();
    await vi.advanceTimersByTimeAsync(0);
    expect(old.getState().host.name).toBe("test-host");
    expect(old.getState().host.headless).toBe(false);
    old.stop();
  });

  it("pings every few seconds, and stops while the connection is closed", async () => {
    const s = newStore();
    await vi.advanceTimersByTimeAsync(PING_INTERVAL_MS * 2);
    expect(transport.callsOf("ping")).toHaveLength(3);
    const answered = s.getState().host.lastPingAt;
    transport.setStatus({ state: "closed", reason: "gone", retryInMs: null });
    await vi.advanceTimersByTimeAsync(PING_INTERVAL_MS * 3);
    expect(transport.callsOf("ping")).toHaveLength(3);
    expect(s.getState().host).toEqual({ name: "test-host", headless: false, lastPingAt: answered });
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
