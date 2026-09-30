// Stage 7c-2: OpenCode calls MCP tools through a code-mode `execute` tool, so
// ACP only shows "execute"; the tools it ran are read from `rawInput.code`
// (the server-side `tool_called` records are shown as well, harness-independent).
// A session started in place of a harness that is no longer installed says so.

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { codeToolCalls } from "../api/acp";
import type { ApiEventBody } from "../api/generated";
import { ev } from "../test/events";
import { TranscriptView } from "../ui/TranscriptView";
import { applyTranscriptEvent, type Transcripts } from "./transcript";

const S = "T-1/implementer";

function toolEvent(seq: number, kind: "tool_call" | "tool_call_update", update: unknown) {
  const body: ApiEventBody = { type: "agent", session: S, event: { type: "output", data: { kind, update } } };
  return ev(seq, body);
}

function fold(events: ReturnType<typeof ev>[]): Transcripts {
  return events.reduce((t, e) => applyTranscriptEvent(t, e), {} as Transcripts);
}

describe("code-mode tool calls", () => {
  it("lists the MCP tools the code calls, once each, in order", () => {
    const code = `const s = await tools.yhtye.get_status({});
await tools.yhtye.report_step_done({ result: "ok" });
await tools.yhtye.get_status({}); tools . nope(); tools.other_server.x ( 1 )`;
    expect(codeToolCalls(code)).toEqual(["yhtye.get_status", "yhtye.report_step_done", "other_server.x"]);
    expect(codeToolCalls(null)).toEqual([]);
    expect(codeToolCalls("ls -la")).toEqual([]);
  });

  it("shows the tools an OpenCode `execute` call ran", () => {
    const t = fold([
      toolEvent(1, "tool_call", { toolCallId: "c1", title: "execute", kind: "other", status: "pending", rawInput: {} }),
      toolEvent(2, "tool_call_update", {
        toolCallId: "c1",
        status: "in_progress",
        rawInput: { code: 'await tools.yhtye.report_step_done({ result: "done" })' },
      }),
      toolEvent(3, "tool_call_update", { toolCallId: "c1", status: "completed" }),
      toolEvent(4, "tool_call", { toolCallId: "c2", title: "bash", kind: "execute", rawInput: { command: "ls" } }),
    ]);
    const items = t[S];
    expect(items).toHaveLength(2);
    expect(items[0]).toMatchObject({ kind: "tool", title: "execute", status: "completed", calls: ["yhtye.report_step_done"] });
    expect(items[1]).toMatchObject({ kind: "tool", title: "bash", calls: [] });
    render(<TranscriptView items={items} agentLabel="implementer" />);
    expect(screen.getByText(/→ yhtye\.report_step_done/)).toBeInTheDocument();
  });
});

describe("a replaced harness", () => {
  it("adds an error line naming what was asked for and what ran", () => {
    const started = (replaced: boolean): ApiEventBody => ({
      type: "session_started",
      session: S,
      role: "implementer",
      task: "T-1",
      pid: 1,
      acp_session_id: "x",
      resumed: false,
      agent: { harness: "claude-code", model: "haiku", effort: null },
      ...(replaced ? { replaced: { harness: "opencode", model: "opencode/free", effort: null } } : {}),
    });
    const t = fold([ev(1, started(true)), ev(2, started(false))]);
    expect(t[S].map((i) => (i.kind === "lifecycle" ? [i.text, i.error] : null))).toEqual([
      ["session started", false],
      ["opencode/opencode/free is not available (not installed?); started claude-code/haiku instead", true],
      ["session started", false],
    ]);
  });
});

describe("a group merged into another branch", () => {
  it("adds nothing to the conversation (the orchestrator reports it)", () => {
    const t = fold([ev(1, { type: "domain", event: { type: "group_base_changed", group: "G-1", from: "feat/x", to: "feat/y" } })]);
    expect(t).toEqual({});
  });
});
