// What the task column shows about a task, derived from the real domain state
// and the task's agent transcripts (design §3.5 mapped onto the domain model:
// progress = steps done, log = the agent's latest activity, footer = the
// agent session and how long the task has been worked on).

import type { Group, Help, State, Task } from "../api/generated";
import type { ProjectView } from "../store/project";
import { ORCHESTRATOR, type TranscriptItem } from "../store/transcript";
import { GROUP_LABEL, isTerminal, type Tone, taskTone } from "./labels";

/** Session keys of a task (`T-1/implementer`, `T-1/review-2`, ...) in start order. */
export function taskSessions(view: ProjectView, task: string): string[] {
  const prefix = `${task}/`;
  const keys = new Set<string>();
  for (const key of Object.keys(view.transcripts)) if (key.startsWith(prefix)) keys.add(key);
  for (const s of view.sessions) if (s.session_key.startsWith(prefix)) keys.add(s.session_key);
  const firstSeq = (k: string) => view.transcripts[k]?.[0]?.seq ?? Number.MAX_SAFE_INTEGER;
  return [...keys].sort((a, b) => firstSeq(a) - firstSeq(b));
}

/** The group the task column is about: the open one, else the latest. */
export function focusGroup(state: State): Group | null {
  const open = state.groups.filter((g) => g.status === "active" || g.status === "finishing");
  if (open.length > 0) return open[open.length - 1];
  return state.groups[state.groups.length - 1] ?? null;
}

export function isOpenGroup(g: Group): boolean {
  return g.status === "active" || g.status === "finishing";
}

/**
 * Every task of the open group has settled (terminal, or can never start) but
 * the orchestrator has not finished the group yet: the work is not merged
 * into the base branch until it calls `finish_group` (Yhtye reminds it, then
 * finishes the group itself — orchestration-model.md §3).
 */
export function awaitingFinish(state: State, group: Group): boolean {
  if (group.status !== "active") return false;
  const tasks = state.tasks.filter((t) => t.group === group.id);
  return tasks.length > 0 && tasks.every((t) => isTerminal(t) || isBlocked(state, t));
}

/** A pending task one of whose dependencies was cancelled (it can never start). */
function isBlocked(state: State, task: Task, depth = 0): boolean {
  if (task.status !== "pending" || depth > state.tasks.length) return false;
  return task.depends_on.some((id) => {
    const dep = state.tasks.find((t) => t.id === id);
    return dep !== undefined && (dep.status === "cancelled" || isBlocked(state, dep, depth + 1));
  });
}

/** The group's status label, telling "all tasks done, not merged yet" apart. */
export function groupLabel(state: State, group: Group): string {
  return awaitingFinish(state, group) ? "完了待ち" : GROUP_LABEL[group.status];
}

export interface GroupProgress {
  done: number;
  total: number;
  handling: number;
}

export function groupProgress(state: State, group: string): GroupProgress {
  const tasks = state.tasks.filter((t) => t.group === group && t.status !== "cancelled");
  return {
    done: tasks.filter((t) => t.status === "done").length,
    total: tasks.length,
    handling: tasks.filter((t) => taskTone(t) === "handling").length,
  };
}

/** Share of the task's steps that are done (a running step counts half). */
export function taskProgress(task: Task): number {
  if (task.steps.length === 0) return 0;
  const done = task.steps.filter((s) => s.status === "done").length;
  const running = task.steps.some((s) => s.status === "running") ? 0.5 : 0;
  return Math.min(1, (done + running) / task.steps.length);
}

/** Running tones show a progress bar and a pulsing badge dot (design §3.5). */
export function isActiveTone(tone: Tone): boolean {
  return tone === "implementing" || tone === "reviewing";
}

function firstLine(text: string, max = 90): string {
  const line = text.trim().split("\n")[0] ?? "";
  return line.length > max ? `${line.slice(0, max)}…` : line;
}

function logLine(item: TranscriptItem): string | null {
  switch (item.kind) {
    case "tool":
      return `${item.title}${item.calls.length > 0 ? ` → ${item.calls.join(", ")}` : ""}${item.status ? ` · ${item.status}` : ""}`;
    case "yhtye_tool":
      return `${item.tool} · ${item.ok ? "ok" : "error"}`;
    case "text":
      return item.textKind === "message" ? firstLine(item.text) : null;
    case "lifecycle":
    case "turn":
      return item.error ? (item.kind === "turn" ? `turn ended: ${item.outcome}` : item.text) : null;
    default:
      return null;
  }
}

/** The latest activity lines of the task's newest session (at most `n`). */
export function taskLog(view: ProjectView, task: string, n = 2): string[] {
  const sessions = taskSessions(view, task);
  const key = sessions[sessions.length - 1];
  if (!key) return [];
  const lines: string[] = [];
  const streamed = view.streaming[key]?.message;
  if (streamed) lines.push(firstLine(streamed));
  const items = view.transcripts[key] ?? [];
  for (let i = items.length - 1; i >= 0 && lines.length < n; i--) {
    const line = logLine(items[i]);
    if (line) lines.push(line);
  }
  return lines.slice(0, n).reverse();
}

/** The session working on the task now (or last), e.g. `implementer`. */
export function taskAgent(view: ProjectView, task: Task): string | null {
  const sessions = taskSessions(view, task.id);
  const key = sessions[sessions.length - 1];
  return key ? (key.split("/")[1] ?? key) : null;
}

/** Milliseconds the task has been worked on (first to last event; to `now` while open). */
export function taskElapsedMs(view: ProjectView, task: Task, now: number): number | null {
  const items = taskSessions(view, task.id).flatMap((k) => view.transcripts[k] ?? []);
  if (items.length === 0) return null;
  const start = Math.min(...items.map((i) => i.ts));
  const end = isTerminal(task) ? Math.max(...items.map((i) => i.ts)) : now;
  return Math.max(0, end - start);
}

export function formatElapsed(ms: number): string {
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return "<1m";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

/** Open help requests of a task. */
export function openHelps(state: State, task: string): Help[] {
  return state.helps.filter((h) => h.task === task && h.state === "open");
}

/** The design's grey meta line under a card, from the real state (or null). */
export function taskMeta(state: State, task: Task, agent: string | null): string | null {
  const group = state.groups.find((g) => g.id === task.group);
  if (task.status === "done" && group && isOpenGroup(group)) {
    if (group.status === "finishing") return "グループを base ブランチへマージ中";
    if (awaitingFinish(state, group)) return "全タスク完了 — オーケストレータのグループ完了 (マージ) 待ち";
    return "同グループの他タスク完了を待機 — 送信保留";
  }
  if (openHelps(state, task.id).length > 0) {
    return `${agent ?? "エージェント"} 待機 · orchestrator 対処中`;
  }
  if (task.status === "awaiting_instruction") return "オーケストレータの指示待ち";
  if (task.status === "pending" && task.depends_on.length > 0) return `待機: ${task.depends_on.join(", ")} の完了`;
  return null;
}

/** Live agent sessions other than the orchestrator (the header's `subagents N`). */
export function liveSubagents(view: ProjectView): number {
  return view.sessions.filter((s) => s.session_key !== ORCHESTRATOR && s.status === "live").length;
}
