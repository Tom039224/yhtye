import { describe, expect, it } from "vitest";

import { durable, FULL_RUN, PROJECT } from "../test/fixtures";
import { agentText, chunk, ev, turnEnded } from "../test/events";
import { applyDurable, applyLive, applySnapshot, newProjectView, type ProjectView } from "./project";
import { ORCHESTRATOR } from "../test/events";

function fold(view: ProjectView, events = FULL_RUN.events): ProjectView {
  return events.reduce((v, e) => (e.live ? applyLive(v, e) : e.seq === v.cursor + 1 ? applyDurable(v, e) : v), view);
}

describe("project view", () => {
  it("builds the orchestrator conversation from the recorded run", () => {
    const view = fold(applySnapshot(newProjectView(PROJECT), FULL_RUN.start));
    expect(view.cursor).toBe(FULL_RUN.end.seq);
    expect(view.state).toEqual(FULL_RUN.end.state);
    const items = view.transcripts[ORCHESTRATOR];
    expect(items.find((i) => i.kind === "user")).toMatchObject({ text: "Add hello.txt please" });
    const texts = items.flatMap((i) => (i.kind === "text" ? [[i.textKind, i.text]] : []));
    expect(texts[0]).toEqual(["thought", "The user wants a greeting file. One code task with a review."]);
    expect(texts[1]).toEqual(["message", "I will create a group for this."]);
    expect(items.find((i) => i.kind === "tool")).toMatchObject({ id: "read-1", title: "Read README.md", status: "completed" });
    expect(items.some((i) => i.kind === "notice" && i.inboxKind === "group_settled")).toBe(true);
    expect(view.transcripts["T-1/implementer"].some((i) => i.kind === "prompt")).toBe(true);
    expect(view.streaming[ORCHESTRATOR]).toEqual({ message: "", thought: "" });
  });

  it("streams chunks and replaces them with the coalesced text", () => {
    let v = newProjectView(PROJECT);
    v = applyLive(v, chunk(0, ORCHESTRATOR, "Hel"));
    v = applyLive(v, chunk(0, ORCHESTRATOR, "lo"));
    v = applyLive(v, chunk(0, ORCHESTRATOR, "hmm", "thought_chunk"));
    expect(v.streaming[ORCHESTRATOR]).toEqual({ message: "Hello", thought: "hmm" });
    v = applyDurable(v, agentText(1, ORCHESTRATOR, "Hello"));
    expect(v.streaming[ORCHESTRATOR]).toEqual({ message: "", thought: "hmm" });
    expect(v.transcripts[ORCHESTRATOR]).toHaveLength(1);
    v = applyDurable(v, turnEnded(2, ORCHESTRATOR));
    expect(v.streaming[ORCHESTRATOR]).toEqual({ message: "", thought: "" });
  });

  it("ignores usage updates and shows other live events without folding them", () => {
    let v = applySnapshot(newProjectView(PROJECT), FULL_RUN.start);
    const usage = ev(0, { type: "agent", session: ORCHESTRATOR, event: { type: "output", data: { kind: "usage", update: {} } } }, true);
    expect(applyLive(v, usage)).toBe(v);
    const unsaved = ev(0, { type: "session_failed", session: "T-1/implementer", error: "boom" }, true);
    v = applyLive(v, unsaved);
    expect(v.cursor).toBe(0);
    expect(v.transcripts["T-1/implementer"]).toEqual([expect.objectContaining({ kind: "lifecycle", error: true })]);
  });

  it("does not fold events already in the snapshot into state", () => {
    const snap = FULL_RUN.end;
    const v = durable(FULL_RUN).reduce(applyDurable, applySnapshot(newProjectView(PROJECT), snap));
    expect(v.state).toEqual(snap.state);
    expect(v.sessions).toEqual(snap.sessions);
    expect(v.transcripts[ORCHESTRATOR].length).toBeGreaterThan(3);
  });

  it("records failed turns and cancellations", () => {
    let v = newProjectView(PROJECT);
    v = applyDurable(v, turnEnded(1, ORCHESTRATOR, "cancelled"));
    v = applyDurable(v, ev(2, { type: "agent", session: ORCHESTRATOR, event: { type: "turn_ended", data: { Err: { code: "closed" } } } }));
    expect(v.transcripts[ORCHESTRATOR]).toEqual([
      expect.objectContaining({ kind: "turn", outcome: "cancelled", error: false }),
      expect.objectContaining({ kind: "turn", outcome: "error (closed)", error: true }),
    ]);
  });
});
