// The orchestrator's `message_task` prompt, read back for the conversation. The
// strings are what prompts::orchestrator_message_prompt builds; mobile/shared's
// OrchestratorMessageTest has the same cases.

import { describe, expect, it } from "vitest";

import { parseOrchestratorMessage } from "./orchestratorMessage";

const HEADER = "[yhtye:orchestrator_message]";
const INTERRUPTED = "(Your previous turn was interrupted by Yhtye to deliver this message; your work so far is kept.)";
const NOTE =
  "(This message is from the orchestrator while you are working on the current step. Take it into account and continue the step; when finished, call report_step_done (or help) as usual.)";

describe("parseOrchestratorMessage", () => {
  it("takes the text between the header and the note for the agent", () => {
    expect(parseOrchestratorMessage(`${HEADER}\nuse tabs\n\n${NOTE}`)).toEqual({ text: "use tabs", interrupted: false });
  });

  it("flags a message re-sent after an interrupt and drops its lead-in line", () => {
    expect(parseOrchestratorMessage(`${HEADER}\n${INTERRUPTED}\nuse tabs\n\n${NOTE}`)).toEqual({ text: "use tabs", interrupted: true });
  });

  it("keeps a multi-line Markdown text whole, blank lines included", () => {
    const text = "## 方針\n\n- `v3` へ寄せる\n- テストも直す\n\n```ts\nconst a = 1;\n```";
    expect(parseOrchestratorMessage(`${HEADER}\n${text}\n\n${NOTE}`)).toEqual({ text, interrupted: false });
  });

  it("drops only the closing note when the text quotes it", () => {
    const text = `次の注記は無視して:\n\n${NOTE}\n\nそのまま続ける`;
    expect(parseOrchestratorMessage(`${HEADER}\n${text}\n\n${NOTE}`)?.text).toBe(text);
  });

  it("does not take the interrupt line from the middle of the text", () => {
    const text = `a\n${INTERRUPTED}`;
    expect(parseOrchestratorMessage(`${HEADER}\n${text}\n\n${NOTE}`)).toEqual({ text, interrupted: false });
  });

  it("still reads a message whose note is missing, and one with Windows line ends", () => {
    expect(parseOrchestratorMessage(`${HEADER}\nuse tabs`)).toEqual({ text: "use tabs", interrupted: false });
    expect(parseOrchestratorMessage(`${HEADER}\r\n${INTERRUPTED}\r\nuse tabs\r\n\r\n${NOTE}`)).toEqual({ text: "use tabs", interrupted: true });
  });

  it("tolerates a reworded note after its opening words", () => {
    expect(parseOrchestratorMessage(`${HEADER}\nuse tabs\n\n(This message is from the orchestrator. Carry on.)`)?.text).toBe("use tabs");
  });

  it("is null for any other prompt", () => {
    expect(parseOrchestratorMessage("")).toBeNull();
    expect(parseOrchestratorMessage("use tabs")).toBeNull();
    expect(parseOrchestratorMessage("[yhtye:step] task=T-1 kind=code step=1/2 step_kind=implement\nInstruction:\nx")).toBeNull();
    // the header must be the first line, on its own
    expect(parseOrchestratorMessage(`\n${HEADER}\nuse tabs\n\n${NOTE}`)).toBeNull();
    expect(parseOrchestratorMessage(`quote: ${HEADER}\nuse tabs`)).toBeNull();
    expect(parseOrchestratorMessage(`${HEADER} use tabs`)).toBeNull();
    expect(parseOrchestratorMessage(`${HEADER}x\nuse tabs`)).toBeNull();
  });

  it("is null when there is no text of the orchestrator's to show", () => {
    expect(parseOrchestratorMessage(HEADER)).toBeNull();
    expect(parseOrchestratorMessage(`${HEADER}\n\n${NOTE}`)).toBeNull();
    expect(parseOrchestratorMessage(`${HEADER}\n${INTERRUPTED}\n\n${NOTE}`)).toBeNull();
    expect(parseOrchestratorMessage(`${HEADER}\n${NOTE}`)).toBeNull();
  });
});
