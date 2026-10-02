// What the sidebar shows about a project's activity, derived from what the
// store already has: the project list (`open`: its orchestration runs in this
// process) and, for the project on screen, its sessions and tasks.

import type { ProjectInfo, TaskStatus } from "../api/generated";
import type { ProjectView } from "../store/project";
import type { Ring } from "./branchTree";

/** A task in these states has an agent (or the merge) working on it. */
const WORKING_TASK: ReadonlySet<TaskStatus> = new Set(["running", "merging"]);

/** A turn is on or a task is being worked on in the project shown. */
export function hasRunningWork(view: ProjectView): boolean {
  if (view.sessions.some((s) => s.status === "live" && s.turn_running)) return true;
  return view.state?.tasks.some((t) => WORKING_TASK.has(t.status)) ?? false;
}

/**
 * Running: work is on in the project shown; ready: its orchestration is up in
 * the core; stopped: it is not open. Only the shown project has its sessions
 * and tasks in the store, so any other one is at most `ready`.
 */
export function projectRing(p: ProjectInfo, view: ProjectView | null): Ring {
  if (!p.open) return "stopped";
  return view?.info.id === p.id && hasRunningWork(view) ? "running" : "ready";
}

/** The projects running in the core in the background: open, but not the one shown. */
export function backgroundProjects(projects: ProjectInfo[], currentId: string | null): ProjectInfo[] {
  return projects.filter((p) => p.open && p.id !== currentId);
}

export const RING_LABEL: Record<Ring, string> = { running: "実行中", ready: "待機中", stopped: "停止" };
