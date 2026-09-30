// The BRANCHES tree's data (orchestrator-desktop §9, Stage 8e): the
// repository's worktrees (named by the branch each has checked out now), each
// with its chats (newest first); chats whose worktree git no longer lists; and
// the local branches no worktree has checked out.

import type { ChatInfo, GitOverview, GitWorktree } from "../api/generated";
import { byRecency, isInternalBranch, orchestratorKey } from "../store/chats";
import type { ProjectView } from "../store/project";

export interface WorktreeNode {
  path: string;
  /** What to call it: its branch now, `detached @sha`, or its directory while git is unread. */
  label: string;
  /** The main worktree (the original clone). */
  isMain: boolean;
  /** Its directory is gone (sending to its chats is refused). */
  missing: boolean;
  chats: ChatInfo[];
}

export interface BranchTree {
  worktrees: WorktreeNode[];
  /** Chats of a worktree git no longer lists (only known once git was read). */
  lost: ChatInfo[];
  /** Local branches checked out nowhere (a chat there makes a Yhtye worktree). */
  otherBranches: string[];
}

/** Where a chat's worktree is now, as git was last read (`unknown`: not read yet). */
export type Place =
  | { kind: "branch"; name: string }
  | { kind: "detached"; sha: string | null }
  | { kind: "missing" }
  | { kind: "unknown" };

export function findWorktree(overview: GitOverview | null, path: string): GitWorktree | undefined {
  return overview?.worktrees.find((w) => w.path === path);
}

export function placeOf(chat: ChatInfo, overview: GitOverview | null): Place {
  if (!overview) return { kind: "unknown" };
  const w = findWorktree(overview, chat.worktree);
  if (!w || w.missing) return { kind: "missing" };
  return w.branch ? { kind: "branch", name: w.branch } : { kind: "detached", sha: w.head_sha };
}

/** The last part of a path (`/a/b/feature-x` → `feature-x`). */
export function dirName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function detachedLabel(sha: string | null): string {
  return sha ? `detached @${sha.slice(0, 7)}` : "detached";
}

/** A place as shown in the header and the pill (`null` while git is unread). */
export function placeLabel(place: Place): string | null {
  switch (place.kind) {
    case "branch":
      return place.name;
    case "detached":
      return detachedLabel(place.sha);
    case "missing":
      return "作業ツリーが見つかりません";
    case "unknown":
      return null;
  }
}

function worktreeLabel(w: GitWorktree): string {
  return w.branch ?? detachedLabel(w.head_sha);
}

/** Yhtye's own worktrees of groups and tasks (on `yhtye/*`); shown only if a chat is bound to one. */
function isShown(w: GitWorktree, chats: ChatInfo[]): boolean {
  if (chats.length > 0) return true;
  return !w.missing && !(w.branch && isInternalBranch(w.branch));
}

export function buildTree(chats: ChatInfo[], overview: GitOverview | null): BranchTree {
  const sorted = byRecency(chats);
  const chatsOf = (path: string) => sorted.filter((c) => c.worktree === path);
  if (!overview) {
    const paths = [...new Set(sorted.map((c) => c.worktree))];
    const worktrees = paths.map((path) => ({ path, label: dirName(path), isMain: false, missing: false, chats: chatsOf(path) }));
    return { worktrees, lost: [], otherBranches: [] };
  }
  const worktrees: WorktreeNode[] = overview.worktrees
    .map((w) => ({ w, chats: chatsOf(w.path) }))
    .filter(({ w, chats: c }) => isShown(w, c))
    .map(({ w, chats: c }) => ({ path: w.path, label: worktreeLabel(w), isMain: w.is_main, missing: w.missing, chats: c }));
  const listed = new Set(overview.worktrees.map((w) => w.path));
  const checkedOut = new Set(overview.worktrees.map((w) => w.branch).filter((b): b is string => b !== null));
  const otherBranches = overview.branches
    .map((b) => b.name)
    .filter((n) => !isInternalBranch(n) && !checkedOut.has(n));
  return { worktrees, lost: sorted.filter((c) => !listed.has(c.worktree)), otherBranches };
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

/**
 * Why a chat cannot be deleted now (`null`: it can). The core decides in the
 * end; this tells the user before asking: its orchestrator is working, or one
 * of its groups is not finished (`active`, `finishing`, `merge_blocked`).
 */
export function deleteBlocker(view: ProjectView, chat: string): string | null {
  if (chatRing(view, chat) === "running") {
    return "実行中のため削除できません。先に停止してください。";
  }
  const open = view.state?.groups.find((g) => g.chat === chat && g.status !== "done" && g.status !== "cancelled");
  return open ? `グループ ${open.id} が終わっていないため削除できません。完了または中止してください。` : null;
}
