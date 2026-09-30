// The BRANCHES tree's data (orchestrator-desktop §9): local branches without
// Yhtye's internal ones, each with its chats (newest first), and the chats
// whose branch no longer exists.

import type { ChatInfo, GitOverview } from "../api/generated";
import { byRecency, isInternalBranch, orchestratorKey } from "../store/chats";
import type { ProjectView } from "../store/project";

export interface BranchNode {
  name: string;
  /** Checked out in the main clone. */
  isHead: boolean;
  chats: ChatInfo[];
}

export interface BranchTree {
  branches: BranchNode[];
  /** Chats of a branch git no longer has (only known once git was read). */
  deleted: ChatInfo[];
}

export function buildTree(chats: ChatInfo[], overview: GitOverview | null): BranchTree {
  const sorted = byRecency(chats);
  const chatsOf = (branch: string) => sorted.filter((c) => c.branch === branch);
  if (!overview) {
    const names = [...new Set(sorted.map((c) => c.branch))];
    return { branches: names.map((name) => ({ name, isHead: false, chats: chatsOf(name) })), deleted: [] };
  }
  const names = overview.branches.map((b) => b.name).filter((n) => !isInternalBranch(n));
  const known = new Set(names);
  return {
    branches: names.map((name) => ({ name, isHead: name === overview.head, chats: chatsOf(name) })),
    deleted: sorted.filter((c) => !known.has(c.branch)),
  };
}

export type Ring = "running" | "ready" | "stopped";

/** Running: a turn is on; ready: the orchestrator process is up; else stopped (lazy start). */
export function chatRing(view: ProjectView, chat: string): Ring {
  const s = view.sessions.find((x) => x.session_key === orchestratorKey(chat));
  if (s?.status !== "live") return "stopped";
  return s.turn_running ? "running" : "ready";
}

/** `3m` / `2h` / `4d` since `ms` (empty for a time in the future or unknown). */
export function shortAge(nowMs: number, ms: number): string {
  const minutes = Math.floor((nowMs - ms) / 60_000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  if (minutes < 60 * 24) return `${Math.floor(minutes / 60)}h`;
  return `${Math.floor(minutes / (60 * 24))}d`;
}
