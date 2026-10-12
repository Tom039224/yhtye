// The prompt that carries the orchestrator's `message_task` message to a task's
// agent (prompts::orchestrator_message_prompt in yhtye-core):
//
//   [yhtye:orchestrator_message]
//   (Your previous turn was interrupted by Yhtye ...)      <- only when re-sent after an interrupt
//   <the orchestrator's text>
//
//   (This message is from the orchestrator while you are ...)   <- a note for the agent
//
// The conversation shows the text itself, labelled as the orchestrator's, and drops
// the note. mobile/shared (`OrchestratorMessage.kt`) parses the same way; keep the two
// in step (their tests share the cases).

/** First line of the prompt (prompts::ORCHESTRATOR_MESSAGE_HEADER). */
export const ORCHESTRATOR_MESSAGE_HEADER = "[yhtye:orchestrator_message]";

/** Start of the interrupt line: only the start is matched, so rewording its tail does not break parsing. */
const INTERRUPT_NOTE_START = "(Your previous turn was interrupted by Yhtye";

/** Start of the closing note for the agent, matched the same way. */
const AGENT_NOTE_START = "(This message is from the orchestrator";

export interface OrchestratorMessage {
  /** What the orchestrator wrote (Markdown), without the lines Yhtye added around it. */
  text: string;
  /** Yhtye cancelled the agent's turn to deliver it (the harness cannot take a message mid-turn). */
  interrupted: boolean;
}

/** The orchestrator's message in `prompt`, or `null` when it is not one (shown as any other prompt then). */
export function parseOrchestratorMessage(prompt: string): OrchestratorMessage | null {
  const lines = prompt.replace(/\r\n/g, "\n").split("\n");
  if (lines[0].trimEnd() !== ORCHESTRATOR_MESSAGE_HEADER) return null;
  const interrupted = lines[1]?.startsWith(INTERRUPT_NOTE_START) ?? false;
  const rest = lines.slice(interrupted ? 2 : 1).join("\n").trim();
  // The note is the last paragraph. The orchestrator's own text may quote it, so only the last one counts.
  const gap = rest.lastIndexOf("\n\n");
  const noteAt = gap < 0 ? 0 : gap + 2;
  const text = (rest.slice(noteAt).startsWith(AGENT_NOTE_START) ? rest.slice(0, noteAt) : rest).trim();
  return text === "" ? null : { text, interrupted };
}
