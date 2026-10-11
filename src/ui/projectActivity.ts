// What the sidebar shows about a project's activity, derived from what the
// store already has: for the project on screen, its sessions and tasks; for any
// other one, the project list (`open`: its orchestration runs in this process;
// `running`: work is on in it, kept current by the store from the core).

import type { ProjectInfo } from "../api/generated";
import { hasRunningWork } from "../store/activity";
import type { ProjectView } from "../store/project";
import type { Ring } from "./branchTree";

/**
 * Running: work is on in the project (a turn or a task); ready: it is not, whether
 * its orchestration is up in the core or starts when the project is opened. The
 * project on screen is judged by its live view, any other one by what the core
 * last said (`running`, which is never set for a project that is not open).
 * Unlike a chat's ring there is no stopped project.
 */
export function projectRing(p: ProjectInfo, view: ProjectView | null): Ring {
  const running = view?.info.id === p.id ? hasRunningWork(view) : p.open && p.running;
  return running ? "running" : "ready";
}

/** The projects running in the core in the background: open, but not the one shown. */
export function backgroundProjects(projects: ProjectInfo[], currentId: string | null): ProjectInfo[] {
  return projects.filter((p) => p.open && p.id !== currentId);
}

export const RING_LABEL: Record<Ring, string> = { running: "実行中", ready: "待機中", stopped: "停止" };
