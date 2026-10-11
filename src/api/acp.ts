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
  /** MCP tools called from code (`server.tool`), empty if none / unknown. */
  calls: string[];
}

/** `ToolCall` / `ToolCallUpdate` (`toolCallId`, `title`, `status`, `kind`, `rawInput`). */
export function toolCallInfo(update: unknown): ToolCallInfo | null {
  const id = str(field(update, "toolCallId"));
  if (id === null) return null;
  return {
    id,
    title: str(field(update, "title")),
    status: str(field(update, "status")),
    kind: str(field(update, "kind")),
    calls: codeToolCalls(str(field(field(update, "rawInput"), "code"))),
  };
}

const CODE_CALL = /\btools\.([A-Za-z_$][\w$-]*)\.([A-Za-z_$][\w$]*)\s*\(/g;

/**
 * The MCP tools a code-mode tool call invokes. OpenCode 2 exposes MCP tools
 * only through its `execute` tool (`rawInput.code` like
 * `await tools.yhtye.report_step_done({...})`), so ACP just shows "execute"
 * (acp-harnesses.md §7.1). Distinct `server.tool` names in order.
 */
export function codeToolCalls(code: string | null): string[] {
  if (!code) return [];
  const out: string[] = [];
  for (const m of code.matchAll(CODE_CALL)) {
    const name = `${m[1]}.${m[2]}`;
    if (!out.includes(name)) out.push(name);
  }
  return out;
}

/** How full a session's context is, from ACP's `usage_update` (tokens). */
export interface ContextUsage {
  used: number;
  size: number;
  /** Cumulative cost of the session, if the agent reports it. */
  cost: { amount: number; currency: string } | null;
}

function count(v: unknown): number | null {
  return typeof v === "number" && Number.isFinite(v) && v >= 0 ? v : null;
}

/** `UsageUpdate` (`used`, `size`, optional `cost: {amount, currency}`); `null` if malformed. */
export function usageInfo(update: unknown): ContextUsage | null {
  const used = count(field(update, "used"));
  const size = count(field(update, "size"));
  if (used === null || size === null) return null;
  const cost = field(update, "cost");
  const amount = field(cost, "amount");
  const currency = str(field(cost, "currency"));
  return {
    used,
    size,
    cost: typeof amount === "number" && Number.isFinite(amount) && currency ? { amount, currency } : null,
  };
}
