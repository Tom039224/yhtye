// Chats of a project (Stage 8): the list the sidebar shows, which one is
// selected, and the per-chat view of the domain state. Pure functions; the
// store (app.ts) calls them.

import type { ApiEvent, ChatInfo, State } from "../api/generated";

/** Branches Yhtye creates for groups and tasks; never shown as work targets. */
export const INTERNAL_BRANCH_PREFIX = "yhtye/";
const ORCHESTRATOR_PREFIX = "orchestrator:";

/** Session key of a chat's orchestrator (`runtime::sessions`). */
export function orchestratorKey(chat: string): string {
  return `${ORCHESTRATOR_PREFIX}${chat}`;
}

export function isOrchestratorKey(session: string): boolean {
  return session.startsWith(ORCHESTRATOR_PREFIX);
}

export function isInternalBranch(name: string): boolean {
  return name.startsWith(INTERNAL_BRANCH_PREFIX);
}

/** Adds a chat unless it is already listed (a response and its event both carry it). */
export function upsertChat(chats: ChatInfo[], chat: ChatInfo): ChatInfo[] {
  return chats.some((c) => c.id === chat.id) ? chats : [...chats, chat];
}

/** Folds the durable events that change the chat list (`ts_ms` gives the times). */
export function applyChatEvent(chats: ChatInfo[], ev: ApiEvent): ChatInfo[] {
  const body = ev.body;
  if (body.type === "prompted" && isOrchestratorKey(body.session)) {
    const id = body.session.slice(ORCHESTRATOR_PREFIX.length);
    return chats.map((c) => (c.id === id ? { ...c, last_used_ms: ev.ts_ms } : c));
  }
  if (body.type !== "domain") return chats;
  const event = body.event;
  if (event.type === "chat_created") {
    const { id, branch, title } = event.chat;
    return upsertChat(chats, { id, branch, title, created_ms: ev.ts_ms, last_used_ms: ev.ts_ms });
  }
  if (event.type === "chat_titled") {
    return chats.map((c) => (c.id === event.chat ? { ...c, title: event.title } : c));
  }
  if (event.type === "chat_branch_changed") {
    return chats.map((c) => (c.id === event.chat ? { ...c, branch: event.to } : c));
  }
  return chats;
}

/** Most recently used first (ties: the newer chat). */
export function byRecency(chats: ChatInfo[]): ChatInfo[] {
  return [...chats].sort((a, b) => b.last_used_ms - a.last_used_ms || b.created_ms - a.created_ms);
}

/** The chat to show when a project opens: the remembered one, else the last used. */
export function initialChat(chats: ChatInfo[], remembered: string | null): string | null {
  if (remembered && chats.some((c) => c.id === remembered)) return remembered;
  return byRecency(chats)[0]?.id ?? null;
}

/** `state` reduced to one chat's groups, their tasks and helps, and its inbox. */
export function scopeState(state: State, chat: string | null): State {
  const groups = state.groups.filter((g) => g.chat === chat);
  const groupIds = new Set(groups.map((g) => g.id));
  const tasks = state.tasks.filter((t) => groupIds.has(t.group));
  const taskIds = new Set(tasks.map((t) => t.id));
  return {
    ...state,
    groups,
    tasks,
    helps: state.helps.filter((h) => taskIds.has(h.task)),
    inbox: state.inbox.filter((e) => e.chat === chat),
  };
}

/** The chat a user's tool call (`cancel_group` / `cancel_task`) concerns. */
export function chatOfUserCall(state: State | null | undefined, args: unknown): string | null {
  if (!state || typeof args !== "object" || args === null) return null;
  const a = args as { group_id?: unknown; task_id?: unknown };
  const task = typeof a.task_id === "string" ? state.tasks.find((t) => t.id === a.task_id) : undefined;
  const group = typeof a.group_id === "string" ? a.group_id : task?.group;
  return state.groups.find((g) => g.id === group)?.chat ?? null;
}
