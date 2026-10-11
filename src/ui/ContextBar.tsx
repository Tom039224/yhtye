import type { ContextUsage } from "../api/acp";
import { Icon } from "./Icon";

/** At or above this share of the context the meter turns to a warning, then to danger. */
const WARN_PERCENT = 70;
const HIGH_PERCENT = 90;

/** What the compact button can do now. */
export type CompactState = { kind: "ready" } | { kind: "compacting" } | { kind: "disabled"; reason: string };

interface Props {
  /** The orchestrator's latest report (`null`: none yet, e.g. right after a restart). */
  usage: ContextUsage | null;
  compact: CompactState;
  onCompact: () => void;
}

/** `84k`, `1.2M`, `950`. */
export function tokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)}k`;
  return String(Math.round(n));
}

/** Share of the context in use, 0–100 (`null` when the size is unknown). */
export function usagePercent(u: ContextUsage): number | null {
  return u.size > 0 ? Math.min(100, Math.round((u.used / u.size) * 100)) : null;
}

function meterText(u: ContextUsage | null): string {
  if (!u) return "—";
  const pct = usagePercent(u);
  const amounts = `${tokens(u.used)} / ${u.size > 0 ? tokens(u.size) : "—"}`;
  return pct === null ? amounts : `${pct}% · ${amounts}`;
}

function meterTitle(u: ContextUsage | null): string {
  if (!u) return "コンテキスト使用量: 未取得 (オーケストレータが報告すると表示されます)";
  const pct = usagePercent(u);
  const size = u.size > 0 ? u.size.toLocaleString("ja-JP") : "不明";
  const lines = [`コンテキスト使用量: ${u.used.toLocaleString("ja-JP")} / ${size} トークン${pct === null ? "" : ` (${pct}%)`}`];
  if (u.cost) lines.push(`累計コスト: ${u.cost.amount.toFixed(2)} ${u.cost.currency}`);
  return lines.join("\n");
}

function tone(u: ContextUsage | null): string {
  const pct = u ? usagePercent(u) : null;
  if (pct === null) return "";
  return pct >= HIGH_PERCENT ? " ctx-meter-high" : pct >= WARN_PERCENT ? " ctx-meter-warn" : "";
}

/**
 * Below the composer, on the right: how full the orchestrator's context is
 * (from the agent's `usage_update`s) and the button that has it compact it.
 */
export function ContextBar({ usage, compact, onCompact }: Props) {
  const pct = usage ? usagePercent(usage) : null;
  const text = meterText(usage);
  const title = meterTitle(usage);
  const compacting = compact.kind === "compacting";
  const buttonTitle =
    compact.kind === "disabled"
      ? compact.reason
      : compacting
        ? "コンテキストを圧縮しています…"
        : "コンテキストを圧縮する (/compact を送信)";
  return (
    <div className="context-bar" data-testid="context-bar">
      <span
        className={`ctx-meter${tone(usage)}${usage ? "" : " ctx-meter-empty"}`}
        title={title}
        aria-label={title}
        role="status"
        data-testid="context-meter"
      >
        <span className="ctx-meter-track" aria-hidden="true">
          <span style={{ width: `${pct ?? 0}%` }} />
        </span>
        <span aria-hidden="true">{text}</span>
      </span>
      <button
        type="button"
        className="ctx-compact"
        title={buttonTitle}
        aria-label={compacting ? "圧縮中…" : "コンテキストを圧縮"}
        disabled={compact.kind !== "ready"}
        onClick={onCompact}
        data-testid="compact-button"
      >
        <Icon name={compacting ? "loader" : "minimize"} size={11} spin={compacting} />
        {compacting ? "圧縮中…" : "圧縮"}
      </button>
    </div>
  );
}
