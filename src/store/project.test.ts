import { describe, expect, it } from "vitest";

import { durable, FULL_RUN, PROJECT } from "../test/fixtures";
import { agentText, chunk, ev, prompted, turnEnded, usage } from "../test/events";
import { applyDurable, applyLive, applySnapshot, forgetChat, newProjectView, type ProjectView, removeChatNow } from "./project";
import { ORCHESTRATOR } from "../test/events";
import { CHAT_INFOS, chatSnapshot } from "../test/chats";
import { orchestratorKey } from "./chats";

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

  it("ignores malformed usage updates and shows other live events without folding them", () => {
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

describe("a deleted chat", () => {
  const deleted = (seq: number, chat: string) => ev(seq, { type: "domain", event: { type: "chat_deleted", chat } });

  /** Both chats loaded, C-2 selected, C-1 with a notification, a sub-agent session and text of each. */
  function twoChats(): ProjectView {
    let v = applySnapshot(newProjectView(PROJECT), chatSnapshot());
    v = {
      ...v,
      unread: { "C-1": true },
      sessions: [
        { session_key: orchestratorKey("C-1"), role: "orchestrator", task: null, acp_session_id: "a", status: "live", turn_running: false, agent: null, cwd: null },
        { session_key: "T-1/implementer", role: "implementer", task: "T-1", acp_session_id: "b", status: "stopped", turn_running: false, agent: null, cwd: null },
        { session_key: "T-2/implementer", role: "implementer", task: "T-2", acp_session_id: "c", status: "live", turn_running: false, agent: null, cwd: null },
      ],
      transcripts: {
        [orchestratorKey("C-1")]: [{ seq: 1, ts: 1, kind: "prompt", text: "x" }],
        "T-1/implementer": [{ seq: 2, ts: 2, kind: "prompt", text: "y" }],
        [orchestratorKey("C-2")]: [{ seq: 3, ts: 3, kind: "prompt", text: "z" }],
      },
      streaming: { [orchestratorKey("C-1")]: { message: "par", thought: "" } },
    };
    return v;
  }

  it("takes its conversation, its tasks' sessions and its notification mark with it", () => {
    const v = applyDurable({ ...twoChats(), stateSeq: 0, cursor: 4 }, deleted(5, "C-1"));
    expect(v.chats.map((c) => c.id)).toEqual(["C-2"]);
    expect(v.state?.chats.map((c) => c.id)).toEqual(["C-2"]);
    expect(v.state?.groups.map((g) => g.id)).toEqual(["G-2"]);
    expect(v.sessions.map((s) => s.session_key)).toEqual(["T-2/implementer"]);
    expect(Object.keys(v.transcripts)).toEqual([orchestratorKey("C-2")]);
    expect(v.streaming).toEqual({});
    expect(v.unread).toEqual({});
    expect(v.selectedChat).toBe("C-2");
  });

  it("shows the most recently used other chat when the shown one is deleted", () => {
    const v = applyDurable({ ...twoChats(), selectedChat: "C-2", stateSeq: 0, cursor: 4 }, deleted(5, "C-2"));
    expect(v.selectedChat).toBe("C-1");
    const last = applyDurable(v, deleted(6, "C-1"));
    expect(last.selectedChat).toBeNull();
    expect(last.chats).toEqual([]);
  });

  it("can be dropped before its event arrives, and the event then changes nothing more", () => {
    const early = removeChatNow(twoChats(), "C-1");
    expect(early.chats).toEqual(CHAT_INFOS.filter((c) => c.id !== "C-1"));
    expect(early.state?.chats.map((c) => c.id)).toContain("C-1");
    expect(early.sessions.map((s) => s.session_key)).toEqual(["T-2/implementer"]);
    const later = applyDurable({ ...early, stateSeq: 0, cursor: 4 }, deleted(5, "C-1"));
    expect(later.state?.chats.map((c) => c.id)).toEqual(["C-2"]);
    expect(later.sessions).toEqual(early.sessions);
    expect(later.transcripts).toEqual(early.transcripts);
    expect(forgetChat(later, null, "C-1")).toEqual(later);
  });

  it("keeps each session's latest context usage until a new session starts", () => {
    let v = newProjectView(PROJECT);
    expect(v.contextUsage[ORCHESTRATOR]).toBeUndefined();
    v = applyLive(v, usage(0, ORCHESTRATOR, { used: 1000, size: 200000 }));
    v = applyLive(v, usage(0, ORCHESTRATOR, { used: 84000, size: 200000, cost: { amount: 1.5, currency: "USD" } }));
    expect(v.contextUsage[ORCHESTRATOR]).toEqual({ used: 84000, size: 200000, cost: { amount: 1.5, currency: "USD" } });
    expect(v.transcripts[ORCHESTRATOR]).toBeUndefined();
    const started = (seq: number, resumed: boolean) =>
      ev(seq, { type: "session_started", session: ORCHESTRATOR, role: "orchestrator", task: null, pid: 1, acp_session_id: "a", resumed, agent: null });
    v = applyDurable(v, started(1, true));
    expect(v.contextUsage[ORCHESTRATOR]).toMatchObject({ used: 84000 });
    v = applyDurable(v, started(2, false));
    expect(v.contextUsage[ORCHESTRATOR]).toBeUndefined();
  });

  it("marks a session compacting from its /compact prompt to the end of that turn", () => {
    let v = newProjectView(PROJECT);
    v = applyDurable(v, prompted(1, ORCHESTRATOR, "[yhtye:user_message]\nhi"));
    expect(v.compacting[ORCHESTRATOR]).toBeUndefined();
    v = applyDurable(v, turnEnded(2, ORCHESTRATOR));
    v = applyDurable(v, prompted(3, ORCHESTRATOR, "/compact"));
    expect(v.compacting[ORCHESTRATOR]).toBe(true);
    // Shown in the conversation; never as the user's message.
    expect(v.transcripts[ORCHESTRATOR]).toEqual([expect.objectContaining({ kind: "compact" })]);
    v = applyDurable(v, turnEnded(4, ORCHESTRATOR));
    expect(v.compacting[ORCHESTRATOR]).toBeUndefined();
  });

  it("forgets a deleted chat's context usage and compaction", () => {
    let v = applySnapshot(newProjectView(PROJECT), chatSnapshot());
    v = applyLive(v, usage(0, ORCHESTRATOR, { used: 5, size: 10 }));
    v = applyDurable(v, prompted(v.cursor + 1, ORCHESTRATOR, "/compact"));
    v = forgetChat(v, v.state, "C-1");
    expect(v.contextUsage).toEqual({});
    expect(v.compacting).toEqual({});
  });
});
