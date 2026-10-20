// Group and task ids (orchestration-model §2.1): names the orchestrator picks
// (`headless-server`, `remote-icon`); older projects have numbered ids
// (`G-<n>`, `T-<n>`), which stay valid. The core derives every branch from them
// (`domain::task_branch`); this reads a branch name back.

import { INTERNAL_BRANCH_PREFIX } from "./chats";

/** Longest group / task name (`MAX_NAME_CHARS` of the core). */
export const MAX_NAME_CHARS = 32;

const NAME = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const NUMBERED_GROUP = /^G-\d+$/;
const NUMBERED_TASK = /^T-\d+$/;
/** An older task's branch `yhtye/<group>-T-<n>`: names are lowercase, so `-T-<n>` at the end is never part of one. */
const NUMBERED_TASK_SUFFIX = /^(.+)-(T-\d+)$/;
/** Between the group and the task in a task branch (names never contain it). */
const TASK_SEPARATOR = "--";

/** A name: `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 32 characters. */
export function isName(id: string): boolean {
  return id.length <= MAX_NAME_CHARS && NAME.test(id);
}

export function isGroupId(id: string): boolean {
  return isName(id) || NUMBERED_GROUP.test(id);
}

export function isTaskId(id: string): boolean {
  return isName(id) || NUMBERED_TASK.test(id);
}

/** What a branch of Yhtye's is for (`null`: not a group's or task's branch). */
export type YhtyeBranch = { kind: "group"; group: string } | { kind: "task"; group: string; task: string };

/**
 * Reads a group / task branch: `yhtye/<group>`, `yhtye/<group>--<task>`, and
 * the older `yhtye/G-<n>-T-<m>` (also a named task in a numbered group:
 * `yhtye/G-46--remote-icon`).
 */
export function parseYhtyeBranch(branch: string): YhtyeBranch | null {
  if (!branch.startsWith(INTERNAL_BRANCH_PREFIX)) return null;
  const rest = branch.slice(INTERNAL_BRANCH_PREFIX.length);
  const split = rest.indexOf(TASK_SEPARATOR);
  if (split >= 0) {
    const group = rest.slice(0, split);
    const task = rest.slice(split + TASK_SEPARATOR.length);
    return isGroupId(group) && isName(task) ? { kind: "task", group, task } : null;
  }
  const numbered = NUMBERED_TASK_SUFFIX.exec(rest);
  if (numbered && isGroupId(numbered[1])) return { kind: "task", group: numbered[1], task: numbered[2] };
  return isGroupId(rest) ? { kind: "group", group: rest } : null;
}
