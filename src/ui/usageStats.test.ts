import { describe, expect, it } from "vitest";

import type { StatsTokens, UsageStats } from "../api/generated";
import { cannedUsageStats } from "../test/memoryTransport";
import {
  breakdownRows,
  dailySeries,
  formatExact,
  formatTokens,
  harnessLabel,
  harnessOrder,
  heatLevel,
  heatmap,
  niceCeil,
  noLogsFound,
  noUsage,
  peak,
  seriesIndex,
  SERIES_COUNT,
  stackBars,
  weekdayOf,
} from "./usageStats";

const tokens = (total: number): StatsTokens => ({ input: total, output: 0, cache_read: 0, cache_creation: 0, total });

/** A report whose days are given as [date, {harness: tokens}]. */
function report(days: [string, Record<string, number>][], over: Partial<UsageStats> = {}): UsageStats {
  const base = cannedUsageStats(1, 0);
  return {
    ...base,
    period_days: days.length,
    days: days.map(([date, byHarness]) => {
      const total = Object.values(byHarness).reduce((n, t) => n + t, 0);
      return {
        date,
        tokens: tokens(total),
        sessions: total > 0 ? 1 : 0,
        harnesses: Object.entries(byHarness).map(([harness, t]) => ({ harness, tokens: t, sessions: 1 })),
      };
    }),
    ...over,
  };
}

describe("formatTokens", () => {
  it("writes small counts as they are and larger ones with K, M, B", () => {
    expect(formatTokens(0)).toBe("0");
    expect(formatTokens(999)).toBe("999");
    expect(formatTokens(1000)).toBe("1K");
    expect(formatTokens(1234)).toBe("1.2K");
    expect(formatTokens(12_345)).toBe("12.3K");
    expect(formatTokens(123_456)).toBe("123K");
    expect(formatTokens(1_500_000)).toBe("1.5M");
    expect(formatTokens(2_500_000_000)).toBe("2.5B");
  });

  it("rounds up into the next unit instead of reading 1000K", () => {
    expect(formatTokens(999_999)).toBe("1M");
    expect(formatTokens(999_999_999)).toBe("1B");
  });

  it("never goes below zero and rounds fractions", () => {
    expect(formatTokens(-5)).toBe("0");
    expect(formatTokens(10.6)).toBe("11");
  });
});

describe("formatExact", () => {
  it("separates thousands", () => {
    expect(formatExact(1234567)).toBe("1,234,567");
    expect(formatExact(0)).toBe("0");
  });
});

describe("harnesses", () => {
  it("names the known ones and shows any other id as it is", () => {
    expect(harnessLabel("claude-code")).toBe("Claude Code");
    expect(harnessLabel("codex")).toBe("Codex");
    expect(harnessLabel("opencode")).toBe("OpenCode");
    expect(harnessLabel("new-agent")).toBe("new-agent");
  });

  it("orders the sources the core looked for first, then others that show up in the data", () => {
    const r = report([["2026-10-19", { zeta: 1, "claude-code": 2, alpha: 3 }]], { by_harness: [{ name: "beta", tokens: tokens(1), sessions: 1, other: false }] });
    expect(harnessOrder(r)).toEqual(["claude-code", "codex", "opencode", "alpha", "beta", "zeta"]);
  });

  it("gives each harness a color index that wraps past the palette", () => {
    const order = Array.from({ length: SERIES_COUNT + 2 }, (_, i) => `h${i}`);
    expect(seriesIndex(order, "h0")).toBe(0);
    expect(seriesIndex(order, `h${SERIES_COUNT}`)).toBe(0);
    expect(seriesIndex(order, "unknown")).toBe((SERIES_COUNT + 2) % SERIES_COUNT);
  });
});

describe("dailySeries", () => {
  it("stacks each day's harnesses in the harness order and leaves out the idle ones", () => {
    const r = report([
      ["2026-10-19", { codex: 30, "claude-code": 70 }],
      ["2026-10-20", {}],
    ]);
    const series = dailySeries(r);
    expect(series.map((d) => d.date)).toEqual(["2026-10-19", "2026-10-20"]);
    expect(series[0]).toMatchObject({ total: 100, sessions: 1, segments: [{ harness: "claude-code", tokens: 70 }, { harness: "codex", tokens: 30 }] });
    expect(series[1]).toMatchObject({ total: 0, segments: [] });
  });
});

describe("the chart's scale", () => {
  it("rounds the peak up to 1, 2 or 5 times a power of ten", () => {
    expect(niceCeil(0)).toBe(1);
    expect(niceCeil(1)).toBe(1);
    expect(niceCeil(1.5)).toBe(2);
    expect(niceCeil(2)).toBe(2);
    expect(niceCeil(3)).toBe(5);
    expect(niceCeil(5.1)).toBe(10);
    expect(niceCeil(730)).toBe(1000);
    expect(niceCeil(1_234_567)).toBe(2_000_000);
  });

  it("takes the tallest day as the peak", () => {
    const series = dailySeries(report([["2026-10-19", { codex: 30 }], ["2026-10-20", { codex: 90 }], ["2026-10-21", {}]]));
    expect(peak(series)).toBe(90);
    expect(peak([])).toBe(0);
  });
});

describe("stackBars", () => {
  it("stands the first segment on the baseline and each next one on the one before", () => {
    const bars = stackBars([{ harness: "a", tokens: 600 }, { harness: "b", tokens: 400 }], 1000, 120);
    expect(bars.map((b) => [b.harness, b.y, b.height])).toEqual([
      ["a", 48, 72],
      ["b", 0, 48],
    ]);
  });

  it("has no bars for a day without usage and does not change its input", () => {
    expect(stackBars([], 1000, 120)).toEqual([]);
    const segments = [{ harness: "a", tokens: 10 }];
    stackBars(segments, 10, 100);
    expect(segments).toEqual([{ harness: "a", tokens: 10 }]);
  });
});

describe("breakdownRows", () => {
  const bucket = (name: string, total: number, other = false) => ({ name, tokens: tokens(total), sessions: 2, other });

  it("gives each row its share of the list's tokens", () => {
    const rows = breakdownRows([bucket("a", 75), bucket("b", 25)]);
    expect(rows.map((r) => [r.name, r.share])).toEqual([["a", 0.75], ["b", 0.25]]);
  });

  it("names the cut-off sum 'その他' and labels and colors harness rows", () => {
    const order = ["claude-code", "codex"];
    const rows = breakdownRows([bucket("codex", 10), bucket("other", 5, true)], harnessLabel, order);
    expect(rows[0]).toMatchObject({ name: "Codex", series: 1, other: false });
    expect(rows[1]).toMatchObject({ name: "その他", series: undefined, other: true });
  });

  it("has no share to give when nothing was used, and keeps same-named rows apart", () => {
    const rows = breakdownRows([bucket("x", 0), bucket("x", 0)]);
    expect(rows.map((r) => r.share)).toEqual([0, 0]);
    expect(new Set(rows.map((r) => r.key)).size).toBe(2);
  });
});

describe("the heatmap", () => {
  it("reads the weekday of a calendar date", () => {
    expect(weekdayOf("2026-10-20")).toBe(2); // a Tuesday
    expect(weekdayOf("2026-10-18")).toBe(0); // the Sunday before
    expect(weekdayOf("2026-02-28")).toBe(6);
  });

  it("cuts the levels at quarters of the busiest day", () => {
    expect(heatLevel(0, 100)).toBe(0);
    expect(heatLevel(5, 0)).toBe(0);
    expect([1, 25, 26, 50, 51, 75, 76, 100].map((t) => heatLevel(t, 100))).toEqual([1, 1, 2, 2, 3, 3, 4, 4]);
  });

  it("lays the days out as a calendar, Sunday first, a column per week", () => {
    // Tuesday the 20th … the Monday after: the first column holds Tue–Sat, the second Sun–Mon.
    const series = dailySeries(
      report([
        ["2026-10-20", { codex: 10 }],
        ["2026-10-21", { codex: 100 }],
        ["2026-10-22", {}],
        ["2026-10-23", {}],
        ["2026-10-24", {}],
        ["2026-10-25", { codex: 50 }],
        ["2026-10-26", {}],
      ]),
    );
    const { cells, weeks } = heatmap(series);
    expect(weeks).toBe(2);
    expect(cells.map((c) => [c.date.slice(8), c.week, c.weekday, c.level])).toEqual([
      ["20", 0, 2, 1],
      ["21", 0, 3, 4],
      ["22", 0, 4, 0],
      ["23", 0, 5, 0],
      ["24", 0, 6, 0],
      ["25", 1, 0, 2],
      ["26", 1, 1, 0],
    ]);
  });

  it("is empty for no days", () => {
    expect(heatmap([])).toEqual({ cells: [], weeks: 0 });
  });
});

describe("the empty states", () => {
  it("tells no logs from no usage", () => {
    const none = report([["2026-10-20", {}]], { sources: [{ harness: "claude-code", found: false, files: 0 }], totals: { tokens: tokens(0), sessions: 0 } });
    expect(noLogsFound(none)).toBe(true);
    const idle = { ...none, sources: [{ harness: "claude-code", found: true, files: 3 }] };
    expect(noLogsFound(idle)).toBe(false);
    expect(noUsage(idle)).toBe(true);
    expect(noUsage(cannedUsageStats(7, 0))).toBe(false);
  });
});
