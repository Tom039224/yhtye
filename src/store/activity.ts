// Whether work is on in a project: what the sidebar's ring turns for. The project
// on screen is judged by its view (instantly); any other one by the `running`
// flag the core reports in the project list, which the store re-reads whenever
// an event of a project not on screen may have changed it.

import type { ApiEvent, ProjectInfo, TaskStatus } from "../api/generated";
import type { ProjectView } from "./project";
import { isOrchestratorKey } from "./chats";

/** A task in these states has an agent (or the merge) working on it. */
const WORKING_TASK: ReadonlySet<TaskStatus> = new Set(["running", "merging"]);

/** A turn is on or a task is being worked on in the project shown. */
export function hasRunningWork(view: ProjectView): boolean {
  if (view.sessions.some((s) => s.status === "live" && s.turn_running)) return true;
  return view.state?.tasks.some((t) => WORKING_TASK.has(t.status)) ?? false;
}

/** One of the shown project's chats has an orchestrator session up (live). */
export function hasLiveOrchestrator(view: ProjectView): boolean {
  return view.sessions.some((s) => s.status === "live" && isOrchestratorKey(s.session_key));
}

/**
 * Whether a stored event may start or end work in its project: a turn starting
 * or ending, a session coming up or going away, a task changing status. The
 * output of a running turn (text, tool calls) does not.
 */
export function changesActivity(ev: ApiEvent): boolean {
  if (ev.live) return false;
  const body = ev.body;
  switch (body.type) {
    case "prompted":
    case "session_started":
    case "session_stopped":
    case "session_interrupted":
      return true;
    case "agent":
      return body.event.type === "turn_ended" || body.event.type === "exited";
    case "domain":
      switch (body.event.type) {
        case "task_created":
        case "task_status_changed":
        case "task_cancelled":
        case "group_cancelled":
        case "chat_deleted":
          return true;
        default:
          return false;
      }
    default:
      return false;
  }
}

/**
 * `projects` with the activity `fresh` (the core's list) reports: `open`,
 * `running` and `awake` of the projects already known, nothing else, so a
 * project opened since the request was made is not lost. The same array if
 * nothing changed.
 */
export function mergeActivity(projects: ProjectInfo[], fresh: ProjectInfo[]): ProjectInfo[] {
  let changed = false;
  const merged = projects.map((p) => {
    const f = fresh.find((x) => x.id === p.id);
    if (!f || (f.open === p.open && f.running === p.running && f.awake === p.awake)) return p;
    changed = true;
    return { ...p, open: f.open, running: f.running, awake: f.awake };
  });
  return changed ? merged : projects;
}

/**
 * `projects` with the running flag and the awake flag of `left` (the project
 * that was on screen) taken from its view, as it is now judged by the flags
 * instead. A view that has not loaded says nothing, and is left as the core
 * reported it.
 */
export function rememberRunning(projects: ProjectInfo[], left: ProjectView | null): ProjectInfo[] {
  if (!left || left.phase !== "ready") return projects;
  const running = hasRunningWork(left);
  const awake = hasLiveOrchestrator(left);
  return projects.map((p) => (p.id === left.info.id && (p.running !== running || p.awake !== awake) ? { ...p, running, awake } : p));
}
