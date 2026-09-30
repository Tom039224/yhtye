// Which icon stands for which state. The words stay (as the tooltip and, for
// assistive tech, hidden text); the icon and its colour carry the meaning.

import type { GroupStatus, HelpKind, StepKind, StepStatus, TaskKind } from "../api/generated";
import { Icon, type IconName } from "./Icon";
import type { Tone } from "./labels";

const TONE_ICON: Record<Tone, IconName> = {
  waiting: "hourglass",
  implementing: "code",
  reviewing: "eye",
  handling: "alert",
  done: "check",
  cancelled: "ban",
};

/** An investigation is "implemented" by searching, not by writing code. */
export function toneIcon(tone: Tone, kind: TaskKind): IconName {
  return tone === "implementing" && kind === "investigate" ? "search" : TONE_ICON[tone];
}

export function stepIcon(step: StepKind, kind: TaskKind): IconName {
  switch (step) {
    case "implement":
      return kind === "investigate" ? "search" : "code";
    case "review":
      return "eye";
    case "checkpoint":
      return "flag";
    case "done":
      return "check";
  }
}

export const STEP_LABEL: Record<StepKind, string> = {
  implement: "実装",
  review: "レビュー",
  checkpoint: "チェックポイント",
  done: "完了",
};

export const STEP_STATUS_LABEL: Record<StepStatus, string> = { pending: "待機", running: "実行中", done: "完了" };

export const KIND_LABEL: Record<TaskKind, string> = { code: "コードの変更", investigate: "調査" };

export function groupIcon(status: GroupStatus, awaitingFinish: boolean): IconName {
  if (awaitingFinish) return "hourglass";
  switch (status) {
    case "active":
      return "layers";
    case "finishing":
      return "git-merge";
    case "done":
      return "check";
    case "merge_blocked":
      return "alert";
    case "cancelled":
      return "ban";
  }
}

export const HELP_ICON: Record<HelpKind, IconName> = {
  blocked: "alert",
  question: "help",
  policy: "help",
  merge_conflict: "git-merge",
  review_rounds_exhausted: "refresh",
  protocol_violation: "alert",
  agent_stopped: "stop",
  agent_crashed: "alert",
  dirty_readonly_tree: "alert",
  git_failed: "git-branch",
};

interface StatusChipProps {
  /** Its colour class (`tone-…` or `group-status-…`). */
  className: string;
  icon: IconName;
  label: string;
  /** Breathes while an agent works. */
  active?: boolean;
  testId?: string;
}

/** A state as a small coloured square holding one icon; the words are its tooltip. */
export function StatusChip({ className, icon, label, active = false, testId }: StatusChipProps) {
  return (
    <span className={`status-chip ${className} ${active ? "status-chip-active" : ""}`} title={label} data-testid={testId}>
      <Icon name={icon} size={13} />
      <span className="sr-only">{label}</span>
    </span>
  );
}

const TOOL_STATUS: Record<string, { icon: IconName; label: string }> = {
  completed: { icon: "check", label: "完了" },
  failed: { icon: "x", label: "失敗" },
  in_progress: { icon: "loader", label: "実行中" },
  pending: { icon: "clock", label: "待機" },
};

/** A tool call's outcome: an icon (a check, a cross, a turning arc), the word hidden. */
export function ToolStatus({ status }: { status: string | null }) {
  const key = status ?? "pending";
  const shown = TOOL_STATUS[key] ?? TOOL_STATUS.pending;
  return (
    <span className={`tool-status tool-status-${key}`} title={shown.label}>
      <Icon name={shown.icon} size={12} spin={key === "in_progress"} />
      <span className="sr-only">{key}</span>
    </span>
  );
}
