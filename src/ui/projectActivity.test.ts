import { describe, expect, it } from "vitest";

import type { ProjectInfo, SessionRecord, Task, TaskStatus } from "../api/generated";
import { emptyState } from "../store/domain";
import { newProjectView, type ProjectView } from "../store/project";
import { FULL_RUN } from "../test/fixtures";
import { backgroundProjects, projectRing } from "./projectActivity";

const info = (id: string, open = true, running = false): ProjectInfo => ({ id, name: id, path: `/work/${id}`, open, running });

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

describe("projectRing", () => {
  it("is ready, not stopped, for a project that is not open (it starts when opened)", () => {
    expect(projectRing(info("a", false), null)).toBe("ready");
    expect(projectRing(info("a", false), viewOf(info("b")))).toBe("ready");
  });

  it("is ready for an open project that is idle", () => {
    expect(projectRing(info("a"), null)).toBe("ready");
    expect(projectRing(info("a"), viewOf(info("a")))).toBe("ready");
    expect(projectRing(info("a"), viewOf(info("b")))).toBe("ready");
  });

  it("is running for the project on screen while work is on in its view", () => {
    const a = info("a");
    expect(projectRing(a, viewOf(a, { sessions: [session({ turn_running: true })] }))).toBe("running");
    expect(projectRing(a, viewOf(a, { tasks: [task("running")] }))).toBe("running");
  });

  it("goes by the view, not the flag, for the project on screen", () => {
    const flagged = info("a", true, true);
    expect(projectRing(flagged, viewOf(flagged))).toBe("ready");
    const idle = info("a");
    expect(projectRing(idle, viewOf(idle, { tasks: [task("merging")] }))).toBe("running");
  });

  it("is running for a project in the background when the core says work is on in it", () => {
    const shown = viewOf(info("b"));
    expect(projectRing(info("a", true, true), shown)).toBe("running");
    expect(projectRing(info("a", true, true), null)).toBe("running");
    expect(projectRing(info("a", true, false), shown)).toBe("ready");
  });

  it("never runs a project that is not open", () => {
    expect(projectRing(info("a", false, true), null)).toBe("ready");
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
