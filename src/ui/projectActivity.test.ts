import { describe, expect, it } from "vitest";

import type { ProjectInfo, SessionRecord, Task, TaskStatus } from "../api/generated";
import { emptyState } from "../store/domain";
import { newProjectView, type ProjectView } from "../store/project";
import { FULL_RUN } from "../test/fixtures";
import { canSleepProject, projectRing } from "./projectActivity";

const info = (id: string, open = true, running = false, awake = false): ProjectInfo => ({ id, name: id, path: `/work/${id}`, open, running, awake });

const session = (over: Partial<SessionRecord> = {}): SessionRecord => ({
  session_key: "orchestrator:C-1",
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
  it("is stopped for a project that is not open (nothing of it is up)", () => {
    expect(projectRing(info("a", false), null)).toBe("stopped");
    expect(projectRing(info("a", false), viewOf(info("b")))).toBe("stopped");
    expect(projectRing(info("a", false, true), null)).toBe("stopped");
  });

  it("is stopped for an open project whose chats all sleep", () => {
    expect(projectRing(info("a"), null)).toBe("stopped");
    expect(projectRing(info("a"), viewOf(info("a")))).toBe("stopped");
    expect(projectRing(info("a"), viewOf(info("b")))).toBe("stopped");
  });

  it("is ready for an open project with an orchestrator up but idle", () => {
    expect(projectRing(info("a", true, false, true), null)).toBe("ready");
    expect(projectRing(info("a", true, false, true), viewOf(info("b")))).toBe("ready");
    expect(projectRing(info("a"), viewOf(info("a"), { sessions: [session()] }))).toBe("ready");
  });

  it("is stopped without a live orchestrator session, even with a task agent up", () => {
    const view = viewOf(info("a"), { sessions: [session({ session_key: "T-1/implementer" })] });
    expect(projectRing(info("a"), view)).toBe("stopped");
  });

  it("is running for the project on screen while work is on in its view", () => {
    const a = info("a");
    expect(projectRing(a, viewOf(a, { sessions: [session({ turn_running: true })] }))).toBe("running");
    expect(projectRing(a, viewOf(a, { tasks: [task("running")] }))).toBe("running");
  });

  it("goes by the view, not the flags, for the project on screen", () => {
    const flagged = info("a", true, true, true);
    expect(projectRing(flagged, viewOf(flagged))).toBe("stopped");
    const idle = info("a");
    expect(projectRing(idle, viewOf(idle, { tasks: [task("merging")] }))).toBe("running");
    const sleeping = info("a", true, false, true);
    expect(projectRing(sleeping, viewOf(sleeping))).toBe("stopped");
  });

  it("is running for a project in the background when the core says work is on in it", () => {
    const shown = viewOf(info("b"));
    expect(projectRing(info("a", true, true), shown)).toBe("running");
    expect(projectRing(info("a", true, true), null)).toBe("running");
    expect(projectRing(info("a", true, false, true), shown)).toBe("ready");
    expect(projectRing(info("a", true, false, false), shown)).toBe("stopped");
  });

  it("does not take another project's work for this one's", () => {
    const shown = viewOf(info("b"), { sessions: [session({ turn_running: true })] });
    expect(projectRing(info("a"), shown)).toBe("stopped");
    expect(projectRing(info("a", true, false, true), shown)).toBe("ready");
  });
});

describe("canSleepProject", () => {
  it("is true for an open project in the background with an orchestrator up and idle", () => {
    expect(canSleepProject(info("a", true, false, true), null)).toBe(true);
    expect(canSleepProject(info("a", true, false, true), viewOf(info("b")))).toBe(true);
  });

  it("is false for a project that is not open or whose chats all sleep", () => {
    expect(canSleepProject(info("a", false), null)).toBe(false);
    expect(canSleepProject(info("a"), viewOf(info("b")))).toBe(false);
  });

  it("is false while work is on in the project, shown or not", () => {
    expect(canSleepProject(info("a", true, true, true), viewOf(info("b")))).toBe(false);
    const a = info("a");
    expect(canSleepProject(a, viewOf(a, { sessions: [session({ turn_running: true })] }))).toBe(false);
    expect(canSleepProject(a, viewOf(a, { tasks: [task("running")] }))).toBe(false);
  });

  it("goes by the view for the project on screen", () => {
    const a = info("a");
    expect(canSleepProject(a, viewOf(a, { sessions: [session()] }))).toBe(true);
    expect(canSleepProject(a, viewOf(a))).toBe(false);
  });
});
