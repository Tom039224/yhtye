// The pieces of the dashboard's usage statistics, drawn with plain SVG and CSS
// (no chart library). The numbers come from `usageStats.ts`; the colors and
// sizes are the stylesheet's (`series-N` for a harness's color, `heat-N` for a
// heatmap level).

import type { UsageStats } from "../api/generated";
import {
  type BreakdownRow,
  type DaySeries,
  formatExact,
  formatTokens,
  harnessLabel,
  heatmap,
  niceCeil,
  peak,
  seriesIndex,
  stackBars,
} from "./usageStats";

/** The width of one day in the daily chart's own coordinates, and the gap between bars. */
const DAY_W = 10;
const BAR_GAP = 2;
/** The height of the daily chart's own coordinates. */
const CHART_H = 120;

/** `2026-10-20` → `10/20`. */
export function shortDate(date: string): string {
  const [, m, d] = date.split("-");
  return `${Number(m)}/${Number(d)}`;
}

const percent = (share: number) => `${Math.round(share * 100)}%`;

/** The totals of the period: tokens by kind, and the sessions. */
export function StatCards({ report }: { report: UsageStats }) {
  const t = report.totals.tokens;
  const cards = [
    { key: "total", label: "合計トークン", value: t.total },
    { key: "input", label: "入力", value: t.input },
    { key: "output", label: "出力", value: t.output },
    { key: "cache-read", label: "キャッシュ読み込み", value: t.cache_read },
    { key: "sessions", label: "セッション", value: report.totals.sessions },
  ];
  return (
    <ul className="stat-cards" aria-label="期間の合計">
      {cards.map((c) => (
        <li key={c.key} className="stat-card" data-stat={c.key}>
          <span className="stat-label">{c.label}</span>
          <strong className="stat-value" title={formatExact(c.value)}>
            {c.key === "sessions" ? formatExact(c.value) : formatTokens(c.value)}
          </strong>
        </li>
      ))}
    </ul>
  );
}

function dayTitle(day: DaySeries): string {
  const lines = [`${day.date}: ${formatExact(day.total)} トークン · ${day.sessions} セッション`];
  for (const s of day.segments) lines.push(`${harnessLabel(s.harness)}: ${formatExact(s.tokens)}`);
  return lines.join("\n");
}

/** Tokens per day, each day a bar stacked by harness (oldest on the left). */
export function DailyChart({ days, order }: { days: DaySeries[]; order: readonly string[] }) {
  const top = niceCeil(peak(days));
  const width = Math.max(1, days.length) * DAY_W;
  const used = order.filter((h) => days.some((d) => d.segments.some((s) => s.harness === h)));
  return (
    <figure className="daily-chart">
      <div className="chart-body">
        <div className="chart-axis" aria-hidden="true">
          <span>{formatTokens(top)}</span>
          <span>{formatTokens(top / 2)}</span>
          <span>0</span>
        </div>
        <svg
          className="chart-svg"
          viewBox={`0 0 ${width} ${CHART_H}`}
          preserveAspectRatio="none"
          role="img"
          aria-label={`日別のトークン数 (${days.length} 日間、ハーネス別)`}
        >
          <line className="chart-grid" x1={0} x2={width} y1={0} y2={0} vectorEffect="non-scaling-stroke" />
          <line className="chart-grid" x1={0} x2={width} y1={CHART_H / 2} y2={CHART_H / 2} vectorEffect="non-scaling-stroke" />
          <line className="chart-grid chart-base" x1={0} x2={width} y1={CHART_H} y2={CHART_H} vectorEffect="non-scaling-stroke" />
          {days.map((day, i) => (
            <g key={day.date} className="chart-day" data-date={day.date} data-total={day.total}>
              <title>{dayTitle(day)}</title>
              <rect className="chart-hit" x={i * DAY_W} y={0} width={DAY_W} height={CHART_H} />
              {stackBars(day.segments, top, CHART_H).map((bar) => (
                <rect
                  key={bar.harness}
                  className={`chart-bar series-${seriesIndex(order, bar.harness)}`}
                  data-harness={bar.harness}
                  x={i * DAY_W + BAR_GAP / 2}
                  y={bar.y}
                  width={DAY_W - BAR_GAP}
                  height={bar.height}
                />
              ))}
            </g>
          ))}
        </svg>
      </div>
      <div className="chart-xlabels" aria-hidden="true">
        <span>{days.length > 0 ? shortDate(days[0].date) : ""}</span>
        <span>{days.length > 0 ? shortDate(days[days.length - 1].date) : ""}</span>
      </div>
      <figcaption className="chart-legend">
        <ul aria-label="ハーネス">
          {used.map((h) => (
            <li key={h} data-harness={h}>
              <span className={`swatch series-${seriesIndex(order, h)}`} aria-hidden="true" />
              {harnessLabel(h)}
            </li>
          ))}
        </ul>
      </figcaption>
    </figure>
  );
}

/** A list of bars: the share of each model / harness / project in the period's tokens. */
export function Breakdown({ title, rows }: { title: string; rows: BreakdownRow[] }) {
  return (
    <section className="breakdown" aria-label={title}>
      <h3 className="breakdown-title">{title}</h3>
      {rows.length === 0 ? (
        <p className="breakdown-empty">データはありません。</p>
      ) : (
        <ul className="breakdown-list">
          {rows.map((r) => (
            <li key={r.key} className={`breakdown-row${r.other ? " other" : ""}`} data-share={r.share.toFixed(4)}>
              <span className="breakdown-name" title={r.name}>
                {r.series !== undefined ? <span className={`swatch series-${r.series}`} aria-hidden="true" /> : null}
                {r.name}
              </span>
              <span className="breakdown-bar" aria-hidden="true">
                <span className={`breakdown-fill${r.series !== undefined ? ` series-${r.series}` : ""}`} style={{ width: `${Math.max(r.share > 0 ? 2 : 0, r.share * 100)}%` }} />
              </span>
              <span className="breakdown-value" title={formatExact(r.tokens.total)}>
                {formatTokens(r.tokens.total)}
              </span>
              <span className="breakdown-share">{percent(r.share)}</span>
              <span className="breakdown-sessions">{formatExact(r.sessions)} セッション</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

const WEEKDAYS = ["日", "月", "火", "水", "木", "金", "土"];
/** The weekdays that carry a label (every other row, so the labels do not crowd). */
const LABELLED_WEEKDAYS = [1, 3, 5];

/** The days as a calendar: a column per week, a row per weekday, a cell's color by the day's tokens. */
export function Heatmap({ days }: { days: DaySeries[] }) {
  const { cells, weeks } = heatmap(days);
  const busiest = days.reduce<DaySeries | null>((best, d) => (best === null || d.total > best.total ? d : best), null);
  const label = busiest && busiest.total > 0 ? `日別の利用ヒートマップ。最も多い日は ${busiest.date} (${formatTokens(busiest.total)} トークン)` : "日別の利用ヒートマップ。利用なし";
  return (
    <figure className="heatmap-figure">
      <div className="heatmap" role="img" aria-label={label} style={{ gridTemplateColumns: `auto repeat(${weeks}, minmax(0, 1fr))` }}>
        {LABELLED_WEEKDAYS.map((d) => (
          <span key={d} className="heat-weekday" style={{ gridColumn: 1, gridRow: d + 1 }} aria-hidden="true">
            {WEEKDAYS[d]}
          </span>
        ))}
        {cells.map((c) => (
          <span
            key={c.date}
            className={`heat-cell heat-${c.level}`}
            data-date={c.date}
            data-level={c.level}
            title={`${c.date}: ${formatExact(c.total)} トークン · ${c.sessions} セッション`}
            style={{ gridColumn: c.week + 2, gridRow: c.weekday + 1 }}
          />
        ))}
      </div>
      <figcaption className="heat-legend" aria-hidden="true">
        少ない
        {[0, 1, 2, 3, 4].map((level) => (
          <span key={level} className={`heat-cell heat-${level}`} />
        ))}
        多い
      </figcaption>
    </figure>
  );
}
