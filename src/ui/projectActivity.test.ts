import { describe, expect, it } from "vitest";

import type { ProjectInfo, SessionRecord, Task, TaskStatus } from "../api/generated";
import { emptyState } from "../store/domain";
import { newProjectView, type ProjectView } from "../store/project";
import { FULL_RUN } from "../test/fixtures";
import { backgroundProjects, hasRunningWork, projectRing } from "./projectActivity";

const info = (id: string, open = true): ProjectInfo => ({ id, name: id, path: `/work/${id}`, open });

const session = (over: Partial<SessionRecord> = {}): SessionRecord => ({
  session_key: "orchestrator/C-1",
  role: "orchestrator",
  task: null,
  acp_session_id: "acp-1",
  status: "live",
  turn_running: false,
  agent: null,
  cwd: null,
  ...over,
});

const task = (status: TaskStatus): Task => ({ ...FULL_RUN.end.state.tasks[0], status });

function viewOf(p: ProjectInfo, over: { sessions?: SessionRecord[]; tasks?: Task[] } = {}): ProjectView {
  const state = { ...emptyState(p.id, { max_review_rounds: 2 }), tasks: over.tasks ?? [] };
  return { ...newProjectView(p), state, sessions: over.sessions ?? [] };
}

describe("hasRunningWork", () => {
  it("is false for a project with nothing going on", () => {
    expect(hasRunningWork(viewOf(info("a")))).toBe(false);
    expect(hasRunningWork(newProjectView(info("a")))).toBe(false);
  });

  it("is true while a live session is in a turn", () => {
    expect(hasRunningWork(viewOf(info("a"), { sessions: [session({ turn_running: true })] }))).toBe(true);
  });

  it("ignores a turn of a session that is not live any more", () => {
    expect(hasRunningWork(viewOf(info("a"), { sessions: [session({ status: "stopped", turn_running: true })] }))).toBe(false);
  });

  it("is true while a task is running or merging, not once it is settled", () => {
    expect(hasRunningWork(viewOf(info("a"), { tasks: [task("running")] }))).toBe(true);
    expect(hasRunningWork(viewOf(info("a"), { tasks: [task("done"), task("merging")] }))).toBe(true);
    expect(hasRunningWork(viewOf(info("a"), { tasks: [task("done"), task("cancelled"), task("pending")] }))).toBe(false);
  });
});

describe("projectRing", () => {
  it("is stopped for a project that is not open", () => {
    expect(projectRing(info("a", false), null)).toBe("stopped");
  });

  it("is ready for an open project that is idle or not on screen", () => {
    expect(projectRing(info("a"), null)).toBe("ready");
    expect(projectRing(info("a"), viewOf(info("a")))).toBe("ready");
  });

  it("is running for the project on screen while work is on", () => {
    const a = info("a");
    expect(projectRing(a, viewOf(a, { sessions: [session({ turn_running: true })] }))).toBe("running");
    expect(projectRing(a, viewOf(a, { tasks: [task("running")] }))).toBe("running");
  });

  it("does not take another project's work for this one's", () => {
    const shown = viewOf(info("b"), { sessions: [session({ turn_running: true })] });
    expect(projectRing(info("a"), shown)).toBe("ready");
  });
});

describe("backgroundProjects", () => {
  const list = [info("a"), info("b"), info("c", false), info("d")];

  it("lists the open projects other than the one on screen, in order", () => {
    expect(backgroundProjects(list, "a").map((p) => p.id)).toEqual(["b", "d"]);
  });

  it("lists every open project when none is on screen", () => {
    expect(backgroundProjects(list, null).map((p) => p.id)).toEqual(["a", "b", "d"]);
  });

  it("never lists a stopped project, and is empty when nothing else runs", () => {
    expect(backgroundProjects([info("a"), info("c", false)], "a")).toEqual([]);
    expect(backgroundProjects([], null)).toEqual([]);
  });
});
