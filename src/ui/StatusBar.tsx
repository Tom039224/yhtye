import type { UsageReport, UsageWindow } from "../api/generated";
import type { UsageView } from "../store/app";
import { useAppState, useStore } from "../store/useStore";
import { Icon } from "./Icon";
import { useNow } from "./useNow";

/**
 * The plan name and the design's usage meters as one group on the right (§3.7);
 * the connection to the core is shown by the sidebar's host footer, not here.
 * Usage comes from the harness through the core (`get_usage`, Stage 6b);
 * without a report the meters stay empty ("—"), never with made-up values.
 * Clicking the meters asks the harness again.
 */
export function StatusBar() {
  const connection = useAppState((s) => s.connection);
  const usage = useAppState((s) => s.usage);
  const store = useStore();
  const now = useNow(30_000);
  const report = usage.report;
  return (
    <footer className="statusbar">
      <span className="spacer" />
      <div className="usage-group" role="group" aria-label="プランと使用量">
        {report?.plan ? (
          <>
            <span className="plan" data-testid="plan" title={`${planLabel(report.plan)} プラン`}>
              {planLabel(report.plan)}
            </span>
            <span className="meter-sep" />
          </>
        ) : null}
        <button
          type="button"
          className="usage"
          title={usageTitle(usage)}
          disabled={connection.state !== "open" || usage.loading}
          onClick={() => void store.refreshUsage(true)}
        >
          <UsageMeter id="5h" label="5h" tone="usage-5h" window={pick(report, "five_hour")} now={now} />
          <span className="meter-sep" />
          <UsageMeter id="week" label="7d" tone="usage-week" window={pick(report, "week")} now={now} />
        </button>
      </div>
    </footer>
  );
}

/** The harness's plan name ("max") as the subscription it is: "Claude Max". */
export function planLabel(plan: string): string {
  const name = plan
    .trim()
    .replace(/^claude\s+/i, "")
    .replace(/(^|\s)\S/g, (c) => c.toUpperCase());
  return `Claude ${name}`;
}

function pick(report: UsageReport | null, kind: UsageWindow["kind"]): UsageWindow | null {
  return report?.windows.find((w) => w.kind === kind) ?? null;
}

function UsageMeter({
  id,
  label,
  tone,
  window,
  now,
}: {
  /** Names the meter for tests (`usage-<id>`); `label` is what is shown. */
  id: string;
  label: string;
  tone: string;
  window: UsageWindow | null;
  now: number;
}) {
  const percent = window ? Math.max(0, Math.min(100, window.percent)) : null;
  return (
    <span className="meter" data-testid={`usage-${id}`}>
      <span className="meter-label">{label}</span>
      <span className="meter-bar">
        {percent !== null ? (
          <span className="meter-fill" style={{ width: `${percent}%`, background: `var(--${tone})` }} />
        ) : null}
      </span>
      <span className="meter-value">{percent !== null ? `${Math.round(percent)}%` : "—"}</span>
      {window?.resets_at_ms != null ? (
        <span className="meter-reset" title="リセットまで">
          <Icon name="hourglass" size={10} />
          {formatRemaining(window.resets_at_ms - now)}
        </span>
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
