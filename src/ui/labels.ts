// Display labels for statuses (orchestration-model.md §4 table; Japanese labels
// from docs/design/orchestrator-desktop.md §3.5).

import type { GroupStatus, HelpKind, Task } from "../api/generated";

/** The design's status families; each has a token color in App.css. */
export type Tone = "waiting" | "implementing" | "reviewing" | "handling" | "done" | "cancelled";

export function taskTone(task: Task): Tone {
  switch (task.status) {
    case "pending":
    case "awaiting_instruction":
      return "waiting";
    case "running": {
      const kind = task.steps[task.current]?.kind;
      return kind === "review" ? "reviewing" : "implementing";
    }
    case "checkpoint":
    case "handling":
    case "interrupted":
      return "handling";
    case "merging":
    case "done":
      return "done";
    case "cancelled":
      return "cancelled";
  }
}

export const TONE_LABEL: Record<Tone, string> = {
  waiting: "待機",
  implementing: "実装中",
  reviewing: "レビュー中",
  handling: "対処中",
  done: "完了",
  cancelled: "中止",
};

export const GROUP_LABEL: Record<GroupStatus, string> = {
  active: "進行中",
  finishing: "マージ中",
  done: "完了",
  cancelled: "中止",
  merge_blocked: "マージ保留",
};

export const HELP_LABEL: Record<HelpKind, string> = {
  blocked: "詰まり",
  question: "質問",
  policy: "方針の相談",
  merge_conflict: "マージコンフリクト",
  review_rounds_exhausted: "レビュー回数超過",
  protocol_violation: "報告なしでターン終了",
  agent_stopped: "エージェント停止",
  agent_crashed: "エージェント異常終了",
  dirty_readonly_tree: "読み取り専用の作業ツリーが変更された",
  git_failed: "git 操作の失敗",
};

export function isTerminal(task: Task): boolean {
  return task.status === "done" || task.status === "cancelled";
}

export function formatTime(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}
