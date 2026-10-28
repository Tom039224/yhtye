// The fake core's `get_usage_stats` and `list_directory` (what the dashboard and the
// project picker are tested against).

import { describe, expect, it } from "vitest";

import { CommandError } from "../api/transport";
import { FULL_RUN, PROJECT } from "./fixtures";
import { FakeCore } from "./memoryTransport";

const core = () => new FakeCore(PROJECT, FULL_RUN.start);

describe("FakeCore get_usage_stats", () => {
  it("answers a canned report with one entry per day of the period", () => {
    for (const days of [7, 30, 365]) {
      const res = core().handle({ type: "get_usage_stats", days, tz_offset_min: 540 });
      if (res.type !== "usage_stats") throw new Error(res.type);
      expect(res.stats.period_days).toBe(days);
      expect(res.stats.tz_offset_min).toBe(540);
      expect(res.stats.days).toHaveLength(days);
      expect(res.stats.days[days - 1].date).toBe("2026-10-20");
      expect(res.stats.days[0].tokens.total).toBe(0);
      expect(res.stats.totals.tokens.total).toBe(res.stats.days.reduce((n, d) => n + d.tokens.total, 0));
    }
  });

  it("defaults to 30 days and serves a report that was set", () => {
    const fake = core();
    const res = fake.handle({ type: "get_usage_stats" });
    if (res.type !== "usage_stats") throw new Error(res.type);
    expect(res.stats.days).toHaveLength(30);
    const custom = { ...res.stats, period_days: 1, days: res.stats.days.slice(-1) };
    fake.usageStats = custom;
    expect(fake.handle({ type: "get_usage_stats", days: 90 })).toEqual({ type: "usage_stats", stats: custom });
  });
});

describe("FakeCore list_directory", () => {
  const list = (cmd: { path?: string; show_hidden?: boolean }) => {
    const res = core().handle({ type: "list_directory", ...cmd });
    if (res.type !== "directory") throw new Error(res.type);
    return res.directory;
  };

  it("starts at the home directory and lists its folders sorted, without hidden ones", () => {
    const home = list({});
    expect(home).toMatchObject({ path: "/home/u", parent: "/home", home: "/home/u" });
    expect(home.entries.map((e) => [e.name, e.is_git_repo])).toEqual([
      ["Documents", false],
      ["repo", true],
    ]);
    expect(home.entries[1].path).toBe("/home/u/repo");
    expect(list({ path: "~" }).path).toBe("/home/u");
    expect(list({ path: "~/repo" }).path).toBe("/home/u/repo");
  });

  it("shows hidden folders on request and the root has no parent", () => {
    expect(list({ path: "/home/u", show_hidden: true }).entries[0].name).toBe(".config");
    expect(list({ path: "/" })).toMatchObject({ parent: null, path: "/" });
  });

  it("refuses what does not exist and relative paths like the core", () => {
    const fail = (path: string) => {
      try {
        core().handle({ type: "list_directory", path });
      } catch (e) {
        return (e as CommandError).code;
      }
      return null;
    };
    expect(fail("/nope")).toBe("not_found");
    expect(fail("some/where")).toBe("invalid_argument");
  });
});
