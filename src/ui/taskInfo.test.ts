import { describe, expect, it } from "vitest";

import { applySnapshot, newProjectView } from "../store/project";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { latestSession, streamedLine, taskLog } from "./taskInfo";

const view = applySnapshot(newProjectView(PROJECT), FULL_RUN.start);
const withItems = {
  ...view,
  transcripts: {
    "T-1/implementer": [
      { seq: 1, ts: 1, kind: "text" as const, textKind: "message" as const, text: "first reply\nmore" },
      { seq: 2, ts: 2, kind: "tool" as const, id: "t1", title: "Read a.txt", status: "completed", toolKind: null, calls: [] },
      { seq: 3, ts: 3, kind: "text" as const, textKind: "message" as const, text: "second reply" },
    ],
  },
};

describe("streamedLine", () => {
  it("is the first line of the message a session streams, cut at 90 characters", () => {
    expect(streamedLine({ s: { message: "one\ntwo", thought: "" } }, "s")).toBe("one");
    expect(streamedLine({ s: { message: "x".repeat(100), thought: "" } }, "s")).toBe(`${"x".repeat(90)}…`);
  });

  it("is null for a session that streams no message, or none", () => {
    expect(streamedLine({ s: { message: "", thought: "thinking" } }, "s")).toBeNull();
    expect(streamedLine({}, "s")).toBeNull();
    expect(streamedLine({ s: { message: "x", thought: "" } }, null)).toBeNull();
  });

  it("keeps an empty first line (the card shows a line for text that is only blank so far)", () => {
    expect(streamedLine({ s: { message: "\n\n", thought: "" } }, "s")).toBe("");
  });
});

describe("taskLog", () => {
  it("shows the streamed line after the latest stored activity, at most two lines", () => {
    expect(taskLog(withItems, "T-1", null)).toEqual([
      { kind: "tool", text: "Read a.txt", status: "completed" },
      { kind: "message", text: "second reply" },
    ]);
    expect(taskLog(withItems, "T-1", "streaming now")).toEqual([
      { kind: "message", text: "second reply" },
      { kind: "message", text: "streaming now" },
    ]);
  });

  it("is empty for a task whose agent has not started", () => {
    expect(latestSession(view, "T-9")).toBeNull();
    expect(taskLog(view, "T-9", "x")).toEqual([]);
  });
});
