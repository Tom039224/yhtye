import { describe, expect, it } from "vitest";

import type { DomainEvent, Help, State, Task } from "../api/generated";
import { chatState } from "../test/chats";
import { CANCEL_RUN, durable, FULL_RUN, type Recording, UNFINISHED_RUN } from "../test/fixtures";
import { applyDomainEvent, emptyState } from "./domain";
import { applySessionEvent } from "./sessions";

function domainEvents(r: Recording, afterSeq = 0): DomainEvent[] {
  return durable(r).flatMap((e) => (e.seq > afterSeq && e.body.type === "domain" ? [e.body.event] : []));
}

describe("the TypeScript reducer agrees with the core", () => {
  for (const [name, rec] of [
    ["run to merge", FULL_RUN],
    ["cancellations", CANCEL_RUN],
    ["group left open, finished by Yhtye", UNFINISHED_RUN],
  ] as const) {
    it(`replays the whole log of "${name}" to the core's final state`, () => {
      const empty = emptyState(rec.end.state.project, rec.end.state.config);
      const state = domainEvents(rec).reduce(applyDomainEvent, empty);
      expect(state).toEqual(rec.end.state);
    });

    it(`folds "${name}" from the start snapshot to the final state and sessions`, () => {
      const events = durable(rec).filter((e) => e.seq > rec.start.seq);
      const state = events.reduce(
        (s, e) => (e.body.type === "domain" ? applyDomainEvent(s, e.body.event) : s),
        rec.start.state,
      );
      const sessions = events.reduce((s, e) => applySessionEvent(s, e.body), rec.start.sessions);
      expect(state).toEqual(rec.end.state);
      expect(sessions).toEqual(rec.end.sessions);
    });
  }

  it("does not mutate the input state", () => {
    const empty = emptyState("p", FULL_RUN.end.state.config);
    const frozen = Object.freeze(empty);
    const events = domainEvents(FULL_RUN);
    expect(() => events.reduce(applyDomainEvent, frozen)).not.toThrow();
    expect(frozen.tasks).toEqual([]);
  });
});

// Transitions the recordings do not cover, checked against the Rust reducer's rules.
describe("constructed domain events", () => {
  const task: Task = {
    id: "T-1",
    group: "G-1",
    title: "t",
    kind: "code",
    depends_on: [],
    instruction: null,
    steps: [
      { kind: "implement", instruction: null, status: "done", result: "r", verdict: null, note: null, nudges: 0 },
      { kind: "review", instruction: null, status: "running", result: null, verdict: null, note: null, nudges: 1 },
      { kind: "done", instruction: null, status: "pending", result: null, verdict: null, note: null, nudges: 0 },
    ],
    current: 1,
    status: "running",
    review_rounds: 0,
    workdir: null,
    cancel_reason: null,
  };
  const base: State = { ...emptyState("p", { max_review_rounds: 2 }), tasks: [task] };
  const help: Help = {
    id: "H-1",
    task: "T-1",
    step: 1,
    kind: "blocked",
    message: "stuck",
    source: { by: "agent", role: "reviewer" },
    state: "open",
    agent_lost: false,
    reply: null,
  };

  it("inserts review steps after the review and counts the round", () => {
    const s = applyDomainEvent(base, {
      type: "review_steps_inserted",
      task: "T-1",
      after: 1,
      steps: [{ kind: "implement", instruction: "fix" }, { kind: "review" }],
    });
    expect(s.tasks[0].steps.map((x) => x.kind)).toEqual(["implement", "review", "implement", "review", "done"]);
    expect(s.tasks[0].steps[2].instruction).toBe("fix");
    expect(s.tasks[0].steps[3].instruction).toBeNull();
    expect(s.tasks[0].review_rounds).toBe(1);
  });

  it("replaces steps from an index", () => {
    const s = applyDomainEvent(base, { type: "steps_replaced", task: "T-1", from: 2, steps: [{ kind: "checkpoint" }, { kind: "done" }] });
    expect(s.tasks[0].steps.map((x) => x.kind)).toEqual(["implement", "review", "checkpoint", "done"]);
  });

  it("raises, answers (resetting the step's nudges), closes and loses helps", () => {
    let s = applyDomainEvent(base, { type: "help_raised", help });
    expect(s.counters.helps).toBe(1);
    s = applyDomainEvent(s, { type: "help_agent_lost", help: "H-1" });
    expect(s.helps[0].agent_lost).toBe(true);
    s = applyDomainEvent(s, { type: "help_answered", help: "H-1", action: "resume", reply: "go on" });
    expect(s.helps[0]).toMatchObject({ state: "answered", reply: "go on" });
    expect(s.tasks[0].steps[1].nudges).toBe(0);
    s = applyDomainEvent(s, { type: "help_closed", help: "H-1" });
    expect(s.helps[0].state).toBe("closed");
  });

  it("counts nudges, resets steps and sets instructions and workdirs", () => {
    let s = applyDomainEvent(base, { type: "nudge_sent", task: "T-1", step: 1 });
    expect(s.tasks[0].steps[1].nudges).toBe(2);
    s = applyDomainEvent(s, { type: "step_reset", task: "T-1", step: 0 });
    expect(s.tasks[0].steps[0].status).toBe("pending");
    s = applyDomainEvent(s, { type: "instruction_set", task: "T-1", instruction: "do it" });
    s = applyDomainEvent(s, { type: "workspace_ready", task: "T-1", path: "/w" });
    expect(s.tasks[0]).toMatchObject({ instruction: "do it", workdir: "/w" });
  });

  it("marks a failed group merge as merge_blocked", () => {
    const group = {
      id: "G-1", chat: "C-1", title: "g", summary: null, base_branch: "main", group_branch: "yhtye/G-1",
      status: "active" as const, finish_summary: null, detail: null, finish_nudges: 0,
    };
    let s = applyDomainEvent(base, { type: "group_created", group });
    s = applyDomainEvent(s, { type: "group_finishing", group: "G-1", summary: "sum" });
    s = applyDomainEvent(s, { type: "group_merge_finished", group: "G-1", ok: false, detail: "dirty" });
    expect(s.groups[0]).toMatchObject({ status: "merge_blocked", detail: "dirty", finish_summary: "sum" });
  });

  it("counts finish reminders, also for groups recorded before the counter existed", () => {
    const { finish_nudges: _, ...old } = {
      id: "G-1", chat: "C-1", title: "g", summary: null, base_branch: "main", group_branch: "yhtye/G-1",
      status: "active" as const, finish_summary: null, detail: null, finish_nudges: 0,
    };
    let s = applyDomainEvent(base, { type: "group_created", group: old as typeof old & { finish_nudges: number } });
    s = applyDomainEvent(s, { type: "group_finish_reminded", group: "G-1" });
    s = applyDomainEvent(s, { type: "group_finish_reminded", group: "G-1" });
    expect(s.groups[0].finish_nudges).toBe(2);
  });

  it("keeps chats bound to their worktree and moves only the group finish_group{into} names", () => {
    const group = (id: string) => ({
      id, chat: "C-1", title: "g", summary: null, base_branch: "feature", group_branch: `yhtye/${id}`,
      status: "merge_blocked" as const, finish_summary: null, detail: null, finish_nudges: 0,
    });
    let s = applyDomainEvent(base, { type: "chat_created", chat: { id: "C-1", worktree: "/wt/feature", title: null } });
    s = applyDomainEvent(s, { type: "group_created", group: group("G-1") });
    s = applyDomainEvent(s, { type: "group_created", group: group("G-2") });
    s = applyDomainEvent(s, { type: "group_base_changed", group: "G-2", from: "feature", to: "renamed" });
    expect(s.chats).toEqual([{ id: "C-1", worktree: "/wt/feature", title: null }]);
    expect(s.groups.map((g) => [g.id, g.base_branch])).toEqual([
      ["G-1", "feature"],
      ["G-2", "renamed"],
    ]);
  });

  it("deletes a chat with its groups, their tasks and helps, and its inbox, and keeps the rest", () => {
    const help: Help = {
      id: "H-1", task: "T-1", step: 0, kind: "blocked", message: "stuck", source: { by: "agent", role: "implementer" },
      state: "open", agent_lost: false, reply: null,
    };
    const entry = (id: number, chat: string) => ({ id, chat, item: { kind: "user_message" as const, attrs: [], body: "hi" } });
    const state: State = {
      ...chatState(),
      helps: [help],
      inbox: [entry(1, "C-1"), entry(2, "C-2")],
      counters: { chats: 2, groups: 2, tasks: 2, helps: 1, inbox: 2 },
    };
    const after = applyDomainEvent(state, { type: "chat_deleted", chat: "C-1" });
    expect(after.chats.map((c) => c.id)).toEqual(["C-2"]);
    expect(after.groups.map((g) => g.id)).toEqual(["G-2"]);
    expect(after.tasks.map((t) => t.id)).toEqual(["T-2"]);
    expect(after.helps).toEqual([]);
    expect(after.inbox.map((e) => e.chat)).toEqual(["C-2"]);
    expect(after.counters).toEqual(state.counters);
    expect(applyDomainEvent(after, { type: "chat_deleted", chat: "C-1" })).toEqual(after);
  });

  it("ignores events for unknown ids", () => {
    expect(applyDomainEvent(base, { type: "task_status_changed", task: "T-9", status: "done" })).toEqual(base);
    expect(applyDomainEvent(base, { type: "step_completed", task: "T-1", step: 7, result: "x", verdict: null })).toEqual(base);
  });
});
