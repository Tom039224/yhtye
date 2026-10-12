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

describe("a session that could not be restored", () => {
  const started = (lost?: string): ApiEventBody => ({
    type: "session_started",
    session: S,
    role: "implementer",
    task: "T-1",
    pid: 1,
    acp_session_id: "new-id",
    resumed: false,
    agent: null,
    ...(lost ? { lost_acp_session_id: lost } : {}),
  });

  it("names the lost ACP session in an error line instead of a plain start", () => {
    const t = fold([ev(1, started("old-id")), ev(2, started())]);
    expect(t[S].map((i) => (i.kind === "lifecycle" ? [i.text, i.error] : null))).toEqual([
      ["could not restore the previous session (old-id); started a new session", true],
      ["session started", false],
    ]);
    render(<TranscriptView items={t[S]} agentLabel="implementer" />);
    expect(screen.getByText(/could not restore the previous session \(old-id\)/)).toBeInTheDocument();
  });
});

describe("a group merged into another branch", () => {
  it("adds nothing to the conversation (the orchestrator reports it)", () => {
    const t = fold([ev(1, { type: "domain", event: { type: "group_base_changed", group: "G-1", from: "feat/x", to: "feat/y" } })]);
    expect(t).toEqual({});
  });
});

describe("the orchestrator's message to a task's agent", () => {
  const HEADER = "[yhtye:orchestrator_message]";
  const INTERRUPTED = "(Your previous turn was interrupted by Yhtye to deliver this message; your work so far is kept.)";
  const NOTE =
    "(This message is from the orchestrator while you are working on the current step. Take it into account and continue the step; when finished, call report_step_done (or help) as usual.)";
  const STEP = "[yhtye:step] task=T-1 kind=code step=1/2 step_kind=implement\nInstruction:\nAppend hello";

  it("is its own item, and any other prompt stays a prompt", () => {
    const t = fold([
      ev(1, { type: "prompted", session: S, text: STEP }),
      ev(2, { type: "prompted", session: S, text: `${HEADER}\n**タブ**を使う\n\n${NOTE}` }),
      ev(3, { type: "prompted", session: S, text: `${HEADER}\n${INTERRUPTED}\n\n${NOTE}` }),
    ]);
    expect(t[S].map((i) => i.kind)).toEqual(["prompt", "orchestrator_message", "prompt"]);
    expect(t[S][1]).toMatchObject({ text: "**タブ**を使う", interrupted: false });
  });

  it("is shown labelled, as Markdown, without the note for the agent", () => {
    const t = fold([ev(1, { type: "prompted", session: S, text: `${HEADER}\n**タブ**を使う\n\n- 一つ目\n\n${NOTE}` })]);
    const { container } = render(<TranscriptView items={t[S]} agentLabel="implementer" />);
    const card = screen.getByLabelText("orchestrator message");
    expect(card).toHaveTextContent("オーケストレータから");
    expect(card.querySelector("strong")?.textContent).toBe("タブ");
    expect(card.querySelectorAll("li")).toHaveLength(1);
    expect(container).not.toHaveTextContent("This message is from the orchestrator");
    expect(container).not.toHaveTextContent(HEADER);
    expect(container.querySelector(".prompt")).toBeNull();
    expect(screen.queryByText("割り込みで再送")).toBeNull();
  });

  it("says when it was sent again after an interrupt", () => {
    const t = fold([ev(1, { type: "prompted", session: S, text: `${HEADER}\n${INTERRUPTED}\nタブを使う\n\n${NOTE}` })]);
    const { container } = render(<TranscriptView items={t[S]} agentLabel="implementer" />);
    expect(screen.getByText("割り込みで再送")).toBeInTheDocument();
    expect(screen.getByLabelText("orchestrator message")).toHaveTextContent("タブを使う");
    expect(container).not.toHaveTextContent("Your previous turn was interrupted");
  });

  it("leaves a step prompt in its folded form", () => {
    const t = fold([ev(1, { type: "prompted", session: S, text: STEP })]);
    const { container } = render(<TranscriptView items={t[S]} agentLabel="implementer" />);
    expect(screen.queryByLabelText("orchestrator message")).toBeNull();
    expect(container.querySelector("details.prompt .prompt-body")?.textContent).toBe(STEP);
  });
});
