// Narrowing of ACP schema payloads, which the generated types leave `unknown`
// (they come from the agent-client-protocol crate; see api/typegen.rs). Only the
// fields the UI shows are read, and each is checked at runtime.

function field(v: unknown, key: string): unknown {
  return typeof v === "object" && v !== null ? (v as Record<string, unknown>)[key] : undefined;
}

function str(v: unknown): string | null {
  return typeof v === "string" ? v : null;
}

/** Text of a `ContentChunk` (`{content: {type: "text", text}}`), if it has text. */
export function chunkText(update: unknown): string | null {
  const content = field(update, "content");
  return field(content, "type") === "text" ? str(field(content, "text")) : null;
}

export interface ToolCallInfo {
  id: string;
  title: string | null;
  status: string | null;
  kind: string | null;
}

/** `ToolCall` / `ToolCallUpdate` (`toolCallId`, `title`, `status`, `kind`). */
export function toolCallInfo(update: unknown): ToolCallInfo | null {
  const id = str(field(update, "toolCallId"));
  if (id === null) return null;
  return {
    id,
    title: str(field(update, "title")),
    status: str(field(update, "status")),
    kind: str(field(update, "kind")),
  };
}
