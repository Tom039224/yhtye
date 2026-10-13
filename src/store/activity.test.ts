import { describe, expect, it } from "vitest";

import type { ApiEvent, ProjectInfo, SessionRecord, Task, TaskStatus } from "../api/generated";
import { agentText, chunk, ev, ORCHESTRATOR, prompted, turnEnded } from "../test/events";
import { FULL_RUN } from "../test/fixtures";
import { changesActivity, hasLiveOrchestrator, hasRunningWork, mergeActivity, rememberRunning } from "./activity";
import { emptyState } from "./domain";
import { newProjectView, type ProjectView } from "./project";

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

function viewOf(
  p: ProjectInfo,
  over: { sessions?: SessionRecord[]; tasks?: Task[]; phase?: ProjectView["phase"] } = {},
): ProjectView {
  const state = { ...emptyState(p.id, { max_review_rounds: 2 }), tasks: over.tasks ?? [] };
  return { ...newProjectView(p), state, sessions: over.sessions ?? [], phase: over.phase ?? "ready" };
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

describe("hasLiveOrchestrator", () => {
  it("is true while one of the chats has an orchestrator session up", () => {
    expect(hasLiveOrchestrator(viewOf(info("a"), { sessions: [session()] }))).toBe(true);
    expect(hasLiveOrchestrator(viewOf(info("a"), { sessions: [session({ status: "stopped" })] }))).toBe(false);
    expect(hasLiveOrchestrator(viewOf(info("a")))).toBe(false);
  });

  it("ignores the sessions of tasks", () => {
    expect(hasLiveOrchestrator(viewOf(info("a"), { sessions: [session({ session_key: "T-1/implementer" })] }))).toBe(false);
  });
});

describe("changesActivity", () => {
  const domain = (type: string): ApiEvent => ev(1, { type: "domain", event: { type, task: "T-1", status: "running" } as never });

  it("is true for a turn starting or ending and for sessions coming up and going away", () => {
    expect(changesActivity(prompted(1, ORCHESTRATOR))).toBe(true);
    expect(changesActivity(turnEnded(1, ORCHESTRATOR))).toBe(true);
    expect(changesActivity(ev(1, { type: "session_stopped", session: ORCHESTRATOR, suspended: false }))).toBe(true);
    expect(changesActivity(ev(1, { type: "session_interrupted", session: ORCHESTRATOR }))).toBe(true);
    expect(changesActivity(ev(1, { type: "agent", session: ORCHESTRATOR, event: { type: "exited", data: { code: 1, signal: null } } }))).toBe(true);
  });

  it("is true for the domain events that move a task in or out of work", () => {
    for (const type of ["task_created", "task_status_changed", "task_cancelled", "group_cancelled", "chat_deleted"]) {
      expect(changesActivity(domain(type))).toBe(true);
    }
    expect(changesActivity(domain("step_completed"))).toBe(false);
  });

  it("is false for the output of a turn and for live events", () => {
    expect(changesActivity(agentText(1, ORCHESTRATOR, "hi"))).toBe(false);
    expect(changesActivity(chunk(1, ORCHESTRATOR, "hi"))).toBe(false);
    expect(changesActivity({ ...prompted(1, ORCHESTRATOR), live: true })).toBe(false);
  });
});

describe("mergeActivity", () => {
  it("takes open, running and awake of the projects it knows, keeping the rest", () => {
    const known = [{ ...info("a"), name: "A" }, info("b", false)];
    const merged = mergeActivity(known, [info("a", true, true, true), info("b", true, false), info("c", true, true)]);
    expect(merged).toEqual([{ ...info("a", true, true, true), name: "A" }, info("b", true, false)]);
  });

  it("notices only awake changing too", () => {
    const known = [info("a", true, false, false)];
    expect(mergeActivity(known, [info("a", true, false, true)])).toEqual([info("a", true, false, true)]);
  });

  it("is the same array when nothing changed, and never drops a project the answer lacks", () => {
    const known = [info("a"), info("b", true, true)];
    expect(mergeActivity(known, [info("b", true, true), info("a")])).toBe(known);
    expect(mergeActivity(known, [])).toBe(known);
  });
});

describe("rememberRunning", () => {
  it("sets the flags of the project left from what its view showed", () => {
    const a = info("a");
    const projects = [a, info("b")];
    const busy = viewOf(a, { tasks: [task("running")] });
    expect(rememberRunning(projects, busy)).toEqual([{ ...a, running: true }, info("b")]);
    const idle = viewOf(info("a", true, true));
    expect(rememberRunning([info("a", true, true)], idle)).toEqual([info("a", true, false)]);
  });

  it("remembers an awake project left while its orchestrator is up", () => {
    const projects = [info("a", true, false, false)];
    const left = viewOf(info("a"), { sessions: [session()] });
    expect(rememberRunning(projects, left)).toEqual([info("a", true, false, true)]);
  });

  it("leaves the list alone when there is no view, or it has not loaded", () => {
    const projects = [info("a", true, true)];
    expect(rememberRunning(projects, null)).toBe(projects);
    expect(rememberRunning(projects, viewOf(info("a"), { phase: "loading" }))).toBe(projects);
    expect(rememberRunning(projects, viewOf(info("a"), { phase: "error" }))).toBe(projects);
  });
});
