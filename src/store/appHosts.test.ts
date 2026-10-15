// What a store knows of the host it speaks to (T-97): the scope of its keys, not
// reopening the last project of a host by itself, the host's version and what
// the shell made of it.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CHAT_LOG, chatSnapshot } from "../test/chats";
import { PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { AppStore, type AppStoreOptions, chatPrefKey, LAST_PROJECT_KEY, USAGE_REFRESH_MS } from "./app";
import { memoryDrafts } from "./drafts";
import { memoryPrefs, type Prefs } from "./prefs";

let transport: MemoryTransport;
let core: FakeCore;
let prefs: Prefs;
let stores: AppStore[];

function newStore(options: AppStoreOptions = {}): AppStore {
  const store = new AppStore(transport, { prefs, ...options });
  store.start();
  stores.push(store);
  return store;
}

const view = (s: AppStore) => {
  const v = s.getState().project;
  if (!v) throw new Error("no project");
  return v;
};

beforeEach(() => {
  transport = new MemoryTransport();
  core = new FakeCore(PROJECT, chatSnapshot());
  core.log = CHAT_LOG;
  core.attach(transport);
  prefs = memoryPrefs();
  stores = [];
});

afterEach(() => {
  for (const s of stores) s.stop();
});

describe("key scope", () => {
  it("is empty for the local core: the keys are what they were before hosts existed", async () => {
    const s = newStore();
    expect(s.keyScope).toBe("");
    expect(s.scopedKey("repo")).toBe("repo");
    await s.openProject(PROJECT.path);
    await vi.waitFor(() => expect(view(s).phase).toBe("ready"));
    s.selectChat("C-1");
    expect(prefs.get(LAST_PROJECT_KEY)).toBe(PROJECT.path);
    expect(prefs.get("yhtye.lastProject")).toBe(PROJECT.path);
    expect(prefs.get(chatPrefKey("repo"))).toBe("C-1");
    expect(prefs.get("yhtye.chat.repo")).toBe("C-1");
  });

  it("goes in front of what a remote host's store saves and of the keys the UI makes from a project id", async () => {
    const s = newStore({ keyScope: "host:sid-1:" });
    expect(s.scopedKey("repo")).toBe("host:sid-1:repo");
    await s.openProject(PROJECT.path);
    await vi.waitFor(() => expect(view(s).phase).toBe("ready"));
    s.selectChat("C-1");
    expect(prefs.get("host:sid-1:yhtye.lastProject")).toBe(PROJECT.path);
    expect(prefs.get("host:sid-1:yhtye.chat.repo")).toBe("C-1");
    expect(prefs.get(LAST_PROJECT_KEY)).toBeNull();
    expect(prefs.get(chatPrefKey("repo"))).toBeNull();
  });

  it("is read back by the same store: the chat of the project it opens, and the project it reopens", async () => {
    prefs.set("host:sid-1:" + LAST_PROJECT_KEY, PROJECT.path);
    prefs.set(chatPrefKey("repo", "host:sid-1:"), "C-1");
    // What the local core remembered is not this host's.
    prefs.set(chatPrefKey("repo"), "C-2");
    const s = newStore({ keyScope: "host:sid-1:" });
    await vi.waitFor(() => expect(view(s).phase).toBe("ready"));
    expect(view(s).selectedChat).toBe("C-1");
  });

  it("does not take the local core's last project for the host's", async () => {
    prefs.set(LAST_PROJECT_KEY, PROJECT.path);
    const s = newStore({ keyScope: "host:sid-1:" });
    await vi.waitFor(() => expect(s.getState().projects).toHaveLength(1));
    expect(s.getState().project).toBeNull();
  });

  it("uses the drafts it is given", () => {
    const drafts = memoryDrafts();
    const s = newStore({ drafts });
    expect(s.drafts).toBe(drafts);
  });
});

describe("reopening the last project", () => {
  it("happens when the connection first comes up", async () => {
    prefs.set(LAST_PROJECT_KEY, PROJECT.path);
    const s = newStore();
    await vi.waitFor(() => expect(view(s).phase).toBe("ready"));
    expect(transport.callsOf("open_project")).toHaveLength(1);
  });

  it("is left to the person for a host: its projects are listed, and nothing is opened there", async () => {
    prefs.set(LAST_PROJECT_KEY, PROJECT.path);
    prefs.set("host:sid-1:" + LAST_PROJECT_KEY, PROJECT.path);
    const s = newStore({ keyScope: "host:sid-1:", reopenLast: false });
    await vi.waitFor(() => expect(s.getState().projects).toHaveLength(1));
    expect(transport.callsOf("open_project")).toEqual([]);
    expect(s.getState().project).toBeNull();
  });
});

describe("the host's version", () => {
  it("is what the host's ping answers", async () => {
    const s = newStore();
    await vi.waitFor(() => expect(s.getState().host.name).toBe("test-host"));
    expect(s.getState().host).toMatchObject({ version: "0.1.0", compatibility: null });
  });

  it("is none when the host says nothing", async () => {
    const serve = transport.handler;
    transport.handler = async (cmd) => {
      const reply = await serve(cmd);
      return reply.type === "pong" ? { ...reply, version: "" } : reply;
    };
    const s = newStore();
    await vi.waitFor(() => expect(s.getState().host.name).toBe("test-host"));
    expect(s.getState().host.version).toBeNull();
  });

  it("keeps what the shell made of it across pings", async () => {
    const s = newStore();
    await vi.waitFor(() => expect(s.getState().host.version).toBe("0.1.0"));
    const incompatible = { type: "incompatible", host: "0.2.0", app: "0.1.0" } as const;
    s.noteHostCompatibility(incompatible);
    expect(s.getState().host.compatibility).toEqual(incompatible);
    const seen = s.getState();
    s.noteHostCompatibility({ ...incompatible });
    expect(s.getState()).toBe(seen);
    // A later ping does not forget it.
    const pings = transport.callsOf("ping").length;
    transport.setStatus({ state: "closed", reason: "x", retryInMs: null });
    transport.setStatus({ state: "open" });
    await vi.waitFor(() => expect(transport.callsOf("ping").length).toBeGreaterThan(pings));
    expect(s.getState().host.compatibility).toEqual(incompatible);
    s.noteHostCompatibility(null);
    expect(s.getState().host.compatibility).toBeNull();
  });
});

describe("showError", () => {
  it("puts a message among the errors on screen", () => {
    const s = newStore();
    s.showError("could not read the hosts");
    expect(s.getState().errors.map((e) => e.message)).toEqual(["could not read the hosts"]);
  });
});

describe("foreground", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("is where the subscription meters are read; a store in the background neither reads nor polls them", async () => {
    vi.useFakeTimers();
    const s = newStore();
    await vi.advanceTimersByTimeAsync(0);
    expect(transport.callsOf("get_usage")).toHaveLength(1);

    s.setForeground(false);
    await vi.advanceTimersByTimeAsync(USAGE_REFRESH_MS * 3);
    expect(transport.callsOf("get_usage")).toHaveLength(1);
    // It stays quiet across a reconnect, too.
    transport.setStatus({ state: "closed", reason: "x", retryInMs: null });
    transport.setStatus({ state: "open" });
    await vi.advanceTimersByTimeAsync(USAGE_REFRESH_MS * 2);
    expect(transport.callsOf("get_usage")).toHaveLength(1);

    s.setForeground(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(transport.callsOf("get_usage")).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(USAGE_REFRESH_MS);
    expect(transport.callsOf("get_usage")).toHaveLength(3);
  });
});
