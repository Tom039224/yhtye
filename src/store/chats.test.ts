import { describe, expect, it } from "vitest";

import { CHAT_INFOS, chatState } from "../test/chats";
import { ev } from "../test/events";
import { applyChatEvent, chatOfUserCall, initialChat, isInternalBranch, orchestratorKey, scopeState } from "./chats";

describe("chat list folding", () => {
  it("adds a created chat once and takes the times from the event", () => {
    const created = ev(9, { type: "domain", event: { type: "chat_created", chat: { id: "C-3", branch: "dev", title: null } } });
    const once = applyChatEvent(CHAT_INFOS, created);
    expect(once.at(-1)).toMatchObject({ id: "C-3", branch: "dev", created_ms: created.ts_ms, last_used_ms: created.ts_ms });
    expect(applyChatEvent(once, created)).toHaveLength(3);
  });

  it("moves a chat to its renamed branch", () => {
    const renamed = ev(9, { type: "domain", event: { type: "chat_branch_changed", chat: "C-2", from: "feat/x", to: "feat/y" } });
    const list = applyChatEvent(CHAT_INFOS, renamed);
    expect(list.find((c) => c.id === "C-2")?.branch).toBe("feat/y");
    expect(list.find((c) => c.id === "C-1")?.branch).toBe("main");
  });

  it("titles a chat and stamps its last use when its orchestrator is prompted", () => {
    const titled = ev(9, { type: "domain", event: { type: "chat_titled", chat: "C-1", title: "Hello" } });
    expect(applyChatEvent(CHAT_INFOS, titled).find((c) => c.id === "C-1")?.title).toBe("Hello");
    const prompted = ev(10, { type: "prompted", session: orchestratorKey("C-1"), text: "x" });
    const list = applyChatEvent(CHAT_INFOS, prompted);
    expect(list.find((c) => c.id === "C-1")?.last_used_ms).toBe(prompted.ts_ms);
    // A sub-agent's prompt is not a chat's use.
    expect(applyChatEvent(CHAT_INFOS, ev(11, { type: "prompted", session: "T-1/implementer", text: "x" }))).toBe(CHAT_INFOS);
  });
});

describe("chat selection and scoping", () => {
  it("picks the remembered chat if it exists, else the most recently used", () => {
    expect(initialChat(CHAT_INFOS, null)).toBe("C-2");
    expect(initialChat(CHAT_INFOS, "C-1")).toBe("C-1");
    expect(initialChat(CHAT_INFOS, "C-9")).toBe("C-2");
    expect(initialChat([], "C-1")).toBeNull();
  });

  it("scopes groups, tasks and the inbox to one chat", () => {
    const s = scopeState(chatState(), "C-2");
    expect(s.groups.map((g) => g.id)).toEqual(["G-2"]);
    expect(s.tasks.map((t) => t.id)).toEqual(["T-2"]);
    expect(scopeState(chatState(), null).groups).toEqual([]);
  });

  it("finds the chat of a user's cancel call from its group or task", () => {
    const state = chatState();
    expect(chatOfUserCall(state, { group_id: "G-2", reason: "r" })).toBe("C-2");
    expect(chatOfUserCall(state, { task_id: "T-1", reason: "r" })).toBe("C-1");
    expect(chatOfUserCall(state, { task_id: "T-9" })).toBeNull();
    expect(chatOfUserCall(null, { task_id: "T-1" })).toBeNull();
  });

  it("recognises Yhtye's internal branches", () => {
    expect(isInternalBranch("yhtye/G-1")).toBe(true);
    expect(isInternalBranch("feat/yhtye")).toBe(false);
  });
});
