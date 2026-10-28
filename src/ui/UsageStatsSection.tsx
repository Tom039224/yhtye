import { useEffect, useState } from "react";

import type { UsageStats } from "../api/generated";
import { useAppState, useStore } from "../store/useStore";
import { EmptyState } from "./EmptyState";
import { Icon, IconButton } from "./Icon";
import { Breakdown, DailyChart, Heatmap, StatCards } from "./UsageCharts";
import {
  breakdownRows,
  dailySeries,
  DEFAULT_PERIOD,
  harnessLabel,
  harnessOrder,
  noLogsFound,
  noUsage,
  PERIODS,
  type Period,
} from "./usageStats";

/**
 * The dashboard's token usage statistics of the host on screen (`get_usage_stats`:
 * what the coding agents on that machine used, read from their own session
 * logs): a period selector, the totals, tokens per day stacked by harness, the
 * by-model / by-harness / by-project breakdowns, and an activity heatmap. A
 * read is made when the section is shown and whenever the period changes; the
 * newest wins. While it is read, the report on screen (if any) stays.
 */
export function UsageStatsSection() {
  const store = useStore();
  const stats = useAppState((s) => s.stats);
  const connected = useAppState((s) => s.connection.state === "open");
  const hostName = useAppState((s) => s.host.name);
  const [days, setDays] = useState<Period>(DEFAULT_PERIOD);
  useEffect(() => {
    if (connected) void store.refreshStats(days);
  }, [store, connected, days]);
  return (
    <section className="dash-section usage-stats" aria-label="利用統計" aria-busy={stats.loading}>
      <header className="dash-section-head">
        <h2 className="dash-heading">
          <Icon name="activity" size={15} />
          利用統計
        </h2>
        {hostName ? <span className="dash-sub">{hostName}</span> : null}
        <span className="spacer" />
        <div className="period-select" role="radiogroup" aria-label="期間">
          {PERIODS.map((p) => (
            <button key={p} type="button" role="radio" aria-checked={p === days} className={`period-option${p === days ? " active" : ""}`} onClick={() => setDays(p)}>
              {p}日
            </button>
          ))}
        </div>
        <IconButton icon="refresh" label="再読み込み" title="ログを読み直す" spin={stats.loading} disabled={!connected} onClick={() => void store.refreshStats(days, true)} />
      </header>
      <StatsBody report={stats.report} error={stats.error} loading={stats.loading || !connected} onRetry={() => void store.refreshStats(days, true)} />
    </section>
  );
}

interface StatsBodyProps {
  report: UsageStats | null;
  error: string | null;
  loading: boolean;
  onRetry: () => void;
}

function StatsBody({ report, error, loading, onRetry }: StatsBodyProps) {
  if (report) return <StatsReport report={report} loading={loading} />;
  if (loading) return <p className="usage-loading" role="status">読み込み中…</p>;
  if (error !== null) {
    return (
      <div className="usage-error" role="alert">
        <EmptyState icon="alert" tone="error" text="利用統計を読めませんでした。" />
        <p className="error-text usage-error-detail">{error}</p>
        <button type="button" className="dash-button" onClick={onRetry}>
          再試行
        </button>
      </div>
    );
  }
  return <p className="usage-loading" role="status">読み込み中…</p>;
}

function StatsReport({ report, loading }: { report: UsageStats; loading: boolean }) {
  if (noLogsFound(report)) {
    return (
      <div className="usage-empty">
        <EmptyState icon="activity" testId="usage-no-logs" text="エージェントのログが見つかりません。" />
        <p className="dash-note">
          {report.sources.map((s) => harnessLabel(s.harness)).join("・")} のセッションログがこのホストにありません。エージェントを使うと、ここに集計されます。
        </p>
      </div>
    );
  }
  if (noUsage(report)) {
    return <EmptyState icon="activity" testId="usage-none" text={`この ${report.period_days} 日間の利用はありません。`} />;
  }
  const order = harnessOrder(report);
  const series = dailySeries(report, order);
  return (
    <div className={`usage-report${loading ? " is-loading" : ""}`}>
      <StatCards report={report} />
      <div className="usage-grid">
        <section className="dash-panel" aria-label="日別のトークン数">
          <h3 className="dash-panel-title">日別のトークン数</h3>
          <DailyChart days={series} order={order} />
        </section>
        <section className="dash-panel" aria-label="アクティビティ">
          <h3 className="dash-panel-title">アクティビティ</h3>
          <Heatmap days={series} />
        </section>
      </div>
      <div className="usage-breakdowns">
        <Breakdown title="モデル別" rows={breakdownRows(report.by_model)} />
        <Breakdown title="ハーネス別" rows={breakdownRows(report.by_harness, harnessLabel, order)} />
        <Breakdown title="プロジェクト別" rows={breakdownRows(report.by_project)} />
      </div>
    </div>
  );
}
