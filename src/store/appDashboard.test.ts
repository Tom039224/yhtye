// What the dashboard asks of the store: the usage statistics, a project opened
// with its refusal handed back to the form, and the host's folders.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { UsageStats } from "../api/generated";
import { CommandError } from "../api/transport";
import { PROJECT, FULL_RUN } from "../test/fixtures";
import { cannedUsageStats, FakeCore, MemoryTransport } from "../test/memoryTransport";
import { AppStore, clientTzOffsetMin } from "./app";

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

afterEach(() => {
  store.stop();
  vi.restoreAllMocks();
});

const stats = () => store.getState().stats;

describe("clientTzOffsetMin", () => {
  it("is the UTC offset in minutes, east positive", () => {
    vi.spyOn(Date.prototype, "getTimezoneOffset").mockReturnValue(-540);
    expect(clientTzOffsetMin()).toBe(540);
    vi.spyOn(Date.prototype, "getTimezoneOffset").mockReturnValue(300);
    expect(clientTzOffsetMin()).toBe(-300);
  });

  it("is 0, not -0, at Greenwich", () => {
    vi.spyOn(Date.prototype, "getTimezoneOffset").mockReturnValue(0);
    expect(Object.is(clientTzOffsetMin(), 0)).toBe(true);
  });
});

describe("AppStore.refreshStats", () => {
  it("starts empty, then holds the report of the period asked, cut with this client's time zone", async () => {
    vi.spyOn(Date.prototype, "getTimezoneOffset").mockReturnValue(-540);
    expect(stats()).toEqual({ report: null, error: null, loading: false });
    const reading = store.refreshStats(7);
    expect(stats().loading).toBe(true);
    await reading;
    expect(transport.callsOf("get_usage_stats")).toEqual([{ type: "get_usage_stats", days: 7, tz_offset_min: 540, refresh: false }]);
    expect(stats().loading).toBe(false);
    expect(stats().error).toBeNull();
    expect(stats().report).toMatchObject({ period_days: 7, tz_offset_min: 540 });
    expect(stats().report?.days).toHaveLength(7);
  });

  it("asks the core to scan again when told to refresh", async () => {
    await store.refreshStats(30, true);
    expect(transport.callsOf("get_usage_stats").at(-1)).toMatchObject({ days: 30, refresh: true });
  });

  it("keeps the report on screen while the next read is on its way", async () => {
    await store.refreshStats(30);
    const before = stats().report;
    core.before = (cmd) => (cmd.type === "get_usage_stats" ? new Promise<void>(() => {}) : undefined);
    void store.refreshStats(90);
    await vi.waitFor(() => expect(stats().loading).toBe(true));
    expect(stats().report).toBe(before);
  });

  it("keeps no report after a failure, and says why", async () => {
    await store.refreshStats(30);
    core.failures.set("get_usage_stats", new CommandError("internal", "cannot read the session logs"));
    await store.refreshStats(30);
    expect(stats()).toEqual({ report: null, error: "cannot read the session logs", loading: false });
    // The failure is the dashboard's to show, not a banner.
    expect(store.getState().errors).toEqual([]);
    core.failures.clear();
    await store.refreshStats(30);
    expect(stats().error).toBeNull();
    expect(stats().report).not.toBeNull();
  });

  it("keeps the answer of the newest read when an older one comes in late", async () => {
    let release = () => {};
    const slow = new Promise<void>((resolve) => {
      release = resolve;
    });
    let first = true;
    core.before = (cmd) => {
      if (cmd.type !== "get_usage_stats") return;
      if (first) {
        first = false;
        return slow;
      }
    };
    const older = store.refreshStats(7);
    await vi.waitFor(() => expect(transport.callsOf("get_usage_stats")).toHaveLength(1));
    await store.refreshStats(90);
    expect(stats().report?.period_days).toBe(90);
    release();
    await older;
    expect(stats().report?.period_days).toBe(90);
    expect(stats().loading).toBe(false);
  });

  it("drops the failure of an older read the same way", async () => {
    let release = () => {};
    const slow = new Promise<void>((resolve) => {
      release = resolve;
    });
    let first = true;
    core.before = async (cmd) => {
      if (cmd.type !== "get_usage_stats") return;
      if (first) {
        first = false;
        await slow;
        throw new CommandError("internal", "too late");
      }
    };
    const older = store.refreshStats(7);
    await vi.waitFor(() => expect(transport.callsOf("get_usage_stats")).toHaveLength(1));
    await store.refreshStats(30);
    release();
    await older;
    expect(stats().error).toBeNull();
    expect(stats().report?.period_days).toBe(30);
  });

  it("serves what the core reports as is", async () => {
    const custom: UsageStats = { ...cannedUsageStats(1, 0), sources: [{ harness: "claude-code", found: false, files: 0 }] };
    core.usageStats = custom;
    await store.refreshStats(7);
    expect(stats().report).toEqual(custom);
  });
});

describe("AppStore.tryOpenProject", () => {
  it("opens the project like openProject", async () => {
    await store.tryOpenProject(PROJECT.path);
    await vi.waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
    expect(transport.callsOf("open_project").at(-1)?.path).toBe(PROJECT.path);
    expect(store.getState().busy.opening).toBe(false);
  });

  it("rejects with the core's refusal instead of putting it in the banners", async () => {
    core.failures.set("open_project", new CommandError("not_found", "/tmp/nope is not a git repository"));
    await expect(store.tryOpenProject("/tmp/nope")).rejects.toThrow("/tmp/nope is not a git repository");
    expect(store.getState().errors).toEqual([]);
    expect(store.getState().busy.opening).toBe(false);
    expect(store.getState().project).toBeNull();
  });

  it("is what openProject shows as a banner when it fails", async () => {
    core.failures.set("open_project", new CommandError("not_found", "/tmp/nope is not a git repository"));
    await store.openProject("/tmp/nope");
    expect(store.getState().errors.map((e) => e.message)).toEqual(["open_project failed: /tmp/nope is not a git repository"]);
    expect(store.getState().busy.opening).toBe(false);
  });
});

describe("AppStore.listDirectory", () => {
  it("lists the host's home when no path is given, and the folder asked for otherwise", async () => {
    const home = await store.listDirectory();
    expect(home).toMatchObject({ path: "/home/u", parent: "/home", home: "/home/u" });
    expect(home.entries.map((e) => [e.name, e.is_git_repo])).toEqual([
      ["Documents", false],
      ["repo", true],
    ]);
    expect(transport.callsOf("list_directory")[0]).toEqual({ type: "list_directory" });
    const repo = await store.listDirectory("/home/u/repo");
    expect(repo.entries.map((e) => e.name)).toEqual(["src"]);
    expect(transport.callsOf("list_directory")[1]).toEqual({ type: "list_directory", path: "/home/u/repo" });
  });

  it("rejects with the core's error", async () => {
    await expect(store.listDirectory("/nope")).rejects.toMatchObject({ code: "not_found" });
    expect(store.getState().errors).toEqual([]);
  });
});
