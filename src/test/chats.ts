// A project with two chats in two worktrees, for the chat UI tests (Stage 8b,
// 8e): C-1 (the main clone on main, older) and C-2 (a Yhtye worktree on feat/x,
// used last) each own a group with a task.

import type { ChatInfo, GitOverview, Snapshot, State } from "../api/generated";
import { orchestratorKey } from "../store/chats";
import { emptyState } from "../store/domain";
import { agentText, userMessage } from "./events";
import { FULL_RUN } from "./fixtures";

const base = FULL_RUN.end.state;
const NOW = Date.now();

/** The main clone and the Yhtye worktree of feat/x. */
export const MAIN_WT = "/repo";
export const FEAT_WT = "/data/worktrees/repo/branches/feat-x";

export const CHAT_INFOS: ChatInfo[] = [
  { id: "C-2", worktree: FEAT_WT, title: "Add feature x", created_ms: NOW - 7_200_000, last_used_ms: NOW - 180_000 },
  { id: "C-1", worktree: MAIN_WT, title: "Refactor checkout", created_ms: NOW - 9_000_000, last_used_ms: NOW - 3_600_000 * 5 },
];

/** Groups G-1 (chat C-1, task T-1) and G-2 (chat C-2, task T-2). */
export function chatState(): State {
  const t1 = { ...base.tasks[0], title: "checkout task", group: "G-1" };
  const t2 = { ...base.tasks[0], id: "T-2", title: "feature task", group: "G-2" };
  const g1 = { ...base.groups[0], id: "G-1", chat: "C-1", title: "Checkout group", base_branch: "main", status: "done" as const };
  const g2 = { ...g1, id: "G-2", chat: "C-2", title: "Feature group", group_branch: "yhtye/G-2", base_branch: "feat/x", status: "active" as const };
  return {
    ...emptyState("repo", { max_review_rounds: 2 }),
    chats: CHAT_INFOS.map(({ id, worktree, title }) => ({ id, worktree, title })).reverse(),
    groups: [g1, g2],
    tasks: [t1, t2],
  };
}

/** One message and one reply per chat (seq 1-4). */
export const CHAT_LOG = [
  userMessage(1, 1, "Refactor checkout", "C-1"),
  userMessage(2, 2, "Add feature x", "C-2"),
  agentText(3, orchestratorKey("C-1"), "Reply about checkout"),
  agentText(4, orchestratorKey("C-2"), "Reply about feature"),
];

export function chatSnapshot(chats: ChatInfo[] = CHAT_INFOS, state: State = chatState()): Snapshot {
  return { seq: CHAT_LOG.length, state, sessions: [], chats };
}

export const CHAT_GIT: GitOverview = {
  head: "main",
  head_sha: "m".repeat(40),
  branches: [
    { name: "main", sha: "m".repeat(40) },
    { name: "feat/x", sha: "f".repeat(40) },
    { name: "yhtye/G-2", sha: "g".repeat(40) },
    { name: "yhtye/G-2-T-2", sha: "t".repeat(40) },
  ],
  worktrees: [
    { path: MAIN_WT, branch: "main", head_sha: "m".repeat(40), is_main: true, missing: false },
    { path: FEAT_WT, branch: "feat/x", head_sha: "f".repeat(40), is_main: false, missing: false },
    { path: "/data/worktrees/repo/G-2/_group", branch: "yhtye/G-2", head_sha: "g".repeat(40), is_main: false, missing: false },
  ],
  commits: [],
  truncated: false,
};
