import type { UsageReport, UsageWindow } from "../api/generated";
import type { UsageView } from "../store/app";
import { useAppState, useStore } from "../store/useStore";
import { useNow } from "./useNow";

/**
 * Plan name and connection to the core on the left; the design's usage meters
 * on the right (§3.7). Usage comes from the harness through the core
 * (`get_usage`, Stage 6b); without a report the meters stay empty ("—"),
 * never with made-up values. Clicking the meters asks the harness again.
 */
export function StatusBar() {
  const connection = useAppState((s) => s.connection);
  const transport = useAppState((s) => s.transport);
  const usage = useAppState((s) => s.usage);
  const store = useStore();
  const now = useNow(30_000);
  const text =
    connection.state === "open"
      ? `接続済み · ${transport.target}`
      : connection.state === "connecting"
        ? `接続中… · ${transport.target}`
        : `切断: ${connection.reason}${connection.retryInMs !== null ? ` · ${Math.round(connection.retryInMs / 100) / 10} 秒後に再接続` : ""}`;
  const dot = connection.state === "open" ? "dot-ok" : connection.state === "connecting" ? "dot-busy" : "dot-bad";
  const report = usage.report;
  return (
    <footer className="statusbar">
      {report?.plan ? (
        <span className="plan" data-testid="plan">
          claude {report.plan}
        </span>
      ) : null}
      <span className="connection" role="status" data-testid="connection">
        <span className={`dot ${dot}`} />
        {text}
      </span>
      <span className="spacer" />
      <button
        type="button"
        className="usage"
        title={usageTitle(usage)}
        disabled={connection.state !== "open" || usage.loading}
        onClick={() => void store.refreshUsage(true)}
      >
        <UsageMeter label="5h" tone="usage-5h" window={pick(report, "five_hour")} now={now} />
        <span className="meter-sep" />
        <UsageMeter label="week" tone="usage-week" window={pick(report, "week")} now={now} />
      </button>
    </footer>
  );
}

function pick(report: UsageReport | null, kind: UsageWindow["kind"]): UsageWindow | null {
  return report?.windows.find((w) => w.kind === kind) ?? null;
}

function UsageMeter({
  label,
  tone,
  window,
  now,
}: {
  label: string;
  tone: string;
  window: UsageWindow | null;
  now: number;
}) {
  const percent = window ? Math.max(0, Math.min(100, window.percent)) : null;
  return (
    <span className="meter" data-testid={`usage-${label}`}>
      <span className="meter-label">{label}</span>
      <span className="meter-bar">
        {percent !== null ? (
          <span className="meter-fill" style={{ width: `${percent}%`, background: `var(--${tone})` }} />
        ) : null}
      </span>
      <span className="meter-value">{percent !== null ? `${Math.round(percent)}%` : "—"}</span>
      {window?.resets_at_ms != null ? (
        <span className="meter-reset">({formatRemaining(window.resets_at_ms - now)})</span>
      ) : null}
    </span>
  );
}

/** `3h 35m` / `2d 4h` / `12m` until the window resets. */
export function formatRemaining(ms: number): string {
  if (ms <= 0) return "リセット済み";
  const minutes = Math.ceil(ms / 60_000);
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  const mins = minutes % 60;
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${mins}m`;
  return `${mins}m`;
}

function usageTitle(usage: UsageView): string {
  if (usage.error) return `使用量を取得できません: ${usage.error}\nクリックで再取得`;
  if (!usage.report) return usage.loading ? "使用量を取得中…" : "使用量はまだ取得していません";
  const lines = usage.report.windows.map((w) => {
    const reset = w.resets_at_ms != null ? ` · リセット ${new Date(w.resets_at_ms).toLocaleString()}` : "";
    return `${w.label}: ${w.percent}%${reset}`;
  });
  const at = new Date(usage.report.fetched_at_ms).toLocaleTimeString();
  return [...lines, `取得 ${at} (Claude Code の /usage) · クリックで再取得`].join("\n");
}
