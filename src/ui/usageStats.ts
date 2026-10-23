// What the dashboard draws of `get_usage_stats` (the core's report of the token
// usage of the coding agents on its machine), as plain data: number formats,
// the series of the daily chart, the breakdown rows and the heatmap's cells.
// Pure, so the shapes and the edge cases are testable without a DOM.

import type { StatsBucket, StatsProject, StatsTokens, UsageStats } from "../api/generated";

/** The periods the dashboard offers, in days. */
export const PERIODS = [7, 30, 90] as const;
export type Period = (typeof PERIODS)[number];
export const DEFAULT_PERIOD: Period = 30;

/** How many series colors the charts have (`series-0` … in dashboard.css); more harnesses share them. */
export const SERIES_COUNT = 6;

const HARNESS_LABEL: Record<string, string> = { "claude-code": "Claude Code", codex: "Codex", opencode: "OpenCode" };

/** A harness id as it is shown. */
export function harnessLabel(id: string): string {
  return HARNESS_LABEL[id] ?? id;
}

/** `1234` → `1.2K`, `1_500_000` → `1.5M`: three significant figures at most. */
export function formatTokens(n: number): string {
  const value = Math.max(0, Math.round(n));
  if (value < 1000) return String(value);
  let unit = value >= 1e9 ? 3 : value >= 1e6 ? 2 : 1;
  let scaled = value / 1000 ** unit;
  // 999_999 would read "1000K": it is "1M".
  if (unit < 3 && Math.round(scaled * 10) / 10 >= 1000) {
    unit += 1;
    scaled = value / 1000 ** unit;
  }
  const digits = scaled < 100 ? Math.round(scaled * 10) / 10 : Math.round(scaled);
  return `${digits}${"KMB"[unit - 1]}`;
}

/** The exact count with thousands separators (tooltips). */
export function formatExact(n: number): string {
  return Math.round(n).toLocaleString("en-US");
}

/**
 * The harnesses in the order the charts give them colors: those the core looked
 * for (`sources`, in its order), then any other that shows up in the data,
 * alphabetically. The same index is the harness's color in every chart.
 */
export function harnessOrder(report: UsageStats): string[] {
  const known = report.sources.map((s) => s.harness);
  const seen = new Set(known);
  const extra = new Set<string>();
  for (const day of report.days) for (const h of day.harnesses) if (!seen.has(h.harness)) extra.add(h.harness);
  for (const b of report.by_harness) if (!seen.has(b.name) && !b.other) extra.add(b.name);
  return [...known, ...[...extra].sort()];
}

/** The `series-N` class index of a harness. */
export function seriesIndex(order: readonly string[], harness: string): number {
  const at = order.indexOf(harness);
  return (at < 0 ? order.length : at) % SERIES_COUNT;
}

export interface DaySeries {
  /** `YYYY-MM-DD`. */
  date: string;
  /** All tokens of the day. */
  total: number;
  sessions: number;
  /** What each harness used that day, in `order` (harnesses with nothing are left out). */
  segments: { harness: string; tokens: number }[];
}

/** The days of the period, oldest first, each with its stack of harnesses in `order`. */
export function dailySeries(report: UsageStats, order: readonly string[] = harnessOrder(report)): DaySeries[] {
  return report.days.map((day) => {
    const byHarness = new Map(day.harnesses.map((h) => [h.harness, h.tokens]));
    const segments = order.flatMap((harness) => {
      const tokens = byHarness.get(harness) ?? 0;
      return tokens > 0 ? [{ harness, tokens }] : [];
    });
    return { date: day.date, total: day.tokens.total, sessions: day.sessions, segments };
  });
}

export interface Bar {
  harness: string;
  /** From the top of the chart. */
  y: number;
  height: number;
}

/**
 * The rectangles of one day's stack in a chart `height` high whose scale tops
 * at `top` tokens: the first segment stands on the baseline, each next one on
 * the one before.
 */
export function stackBars(segments: readonly { harness: string; tokens: number }[], top: number, height: number): Bar[] {
  return segments.reduce<{ bars: Bar[]; used: number }>(
    (acc, s) => {
      const h = (s.tokens / top) * height;
      return { bars: [...acc.bars, { harness: s.harness, y: height - acc.used - h, height: h }], used: acc.used + h };
    },
    { bars: [], used: 0 },
  ).bars;
}

/** `max` rounded up to 1, 2, 5 × 10^k: the top of the chart's scale (1 for no usage at all). */
export function niceCeil(max: number): number {
  if (!(max > 0)) return 1;
  const magnitude = 10 ** Math.floor(Math.log10(max));
  const f = max / magnitude;
  const step = f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10;
  return step * magnitude;
}

/** The tallest stack of the days (the bar chart's scale is {@link niceCeil} of it). */
export function peak(days: readonly DaySeries[]): number {
  return days.reduce((m, d) => Math.max(m, d.total), 0);
}

export interface BreakdownRow {
  key: string;
  name: string;
  tokens: StatsTokens;
  sessions: number;
  /** Of the tokens of the whole list, 0..1. */
  share: number;
  /** The sum of the entries the core cut from a long list. */
  other: boolean;
  /** The harness's chart color, for a harness row. */
  series?: number;
}

type Bucket = Pick<StatsBucket, "tokens" | "sessions" | "other"> & { name: string; project?: StatsProject["project"] };

/** Rows for a list of buckets (the core sorts them, most tokens first): each with its share of the list's total. */
export function breakdownRows(buckets: readonly Bucket[], label: (name: string) => string = (n) => n, order?: readonly string[]): BreakdownRow[] {
  const total = buckets.reduce((n, b) => n + b.tokens.total, 0);
  return buckets.map((b, i) => ({
    key: `${i}:${b.project ?? ""}:${b.name}`,
    name: b.other ? "その他" : label(b.name),
    tokens: b.tokens,
    sessions: b.sessions,
    share: total > 0 ? b.tokens.total / total : 0,
    other: b.other,
    series: order && !b.other ? seriesIndex(order, b.name) : undefined,
  }));
}

/** The weekday (0 = Sunday) of a calendar date `YYYY-MM-DD`. */
export function weekdayOf(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(Date.UTC(y, m - 1, d)).getUTCDay();
}

export type HeatLevel = 0 | 1 | 2 | 3 | 4;

/** 0 for a day without usage; 1…4 by the day's share of the busiest day (quarters). */
export function heatLevel(total: number, max: number): HeatLevel {
  if (!(total > 0) || !(max > 0)) return 0;
  const ratio = total / max;
  if (ratio <= 0.25) return 1;
  if (ratio <= 0.5) return 2;
  if (ratio <= 0.75) return 3;
  return 4;
}

export interface HeatCell {
  date: string;
  total: number;
  sessions: number;
  level: HeatLevel;
  /** Week column from 0 (the week of the first day), weekday row from 0 (Sunday). */
  week: number;
  weekday: number;
}

/** The days laid out like a calendar: one column per week, one row per weekday, Sunday first. */
export function heatmap(days: readonly DaySeries[]): { cells: HeatCell[]; weeks: number } {
  if (days.length === 0) return { cells: [], weeks: 0 };
  const max = peak(days);
  const lead = weekdayOf(days[0].date);
  const cells = days.map((d, i): HeatCell => {
    const slot = lead + i;
    return { date: d.date, total: d.total, sessions: d.sessions, level: heatLevel(d.total, max), week: Math.floor(slot / 7), weekday: slot % 7 };
  });
  return { cells, weeks: cells[cells.length - 1].week + 1 };
}

/** The core found no logs of any harness on its machine. */
export function noLogsFound(report: UsageStats): boolean {
  return !report.sources.some((s) => s.found);
}

/** Logs exist but nothing was used in the period. */
export function noUsage(report: UsageStats): boolean {
  return report.totals.tokens.total === 0 && report.totals.sessions === 0;
}
