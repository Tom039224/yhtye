import { describe, expect, it } from "vitest";

import type { Group, Task } from "../api/generated";
import { DEFAULT_EXCHANGES, exchangesFrom, markdownOf, MAX_EXCHANGES, mixFrom, realLog } from "./benchChat";
import { FULL_RUN } from "./fixtures";

describe("exchangesFrom", () => {
  it("is the layout checks' 160 round trips unless asked", () => {
    expect(exchangesFrom("")).toBe(DEFAULT_EXCHANGES);
    expect(exchangesFrom("?scenario=long")).toBe(160);
  });

  it("takes a whole number from the URL", () => {
    expect(exchangesFrom("?scenario=long&exchanges=1000")).toBe(1000);
  });

  it("falls back to the default for anything else", () => {
    for (const bad of ["abc", "0", "-5", "1.5", "", `${MAX_EXCHANGES + 1}`]) expect(exchangesFrom(`?exchanges=${bad}`)).toBe(DEFAULT_EXCHANGES);
  });
});

describe("mixFrom", () => {
  it("is plain unless asked for the real mix", () => {
    expect(mixFrom("?scenario=long")).toBe("plain");
    expect(mixFrom("?mix=other")).toBe("plain");
    expect(mixFrom("?scenario=long&mix=real")).toBe("real");
  });
});

describe("realLog", () => {
  const base = FULL_RUN.end.state;
  const log = realLog({ exchanges: 60, chat: "C-1", orchestrator: "orchestrator:C-1", group: base.groups[0] as Group, taskIds: ["T-1", "T-2"], task: base.tasks[0] as Task, attachments: [] });

  it("numbers the events from 1 without gaps", () => {
    expect(log.map((e) => e.seq)).toEqual(log.map((_, i) => i + 1));
    expect(log.every((e) => !e.live)).toBe(true);
  });

  it("holds one message of the user per round trip, and the kinds of content a real chat has", () => {
    const users = log.filter((e) => e.body.type === "domain" && e.body.event.type === "inbox_queued" && e.body.event.entry.item.kind === "user_message");
    expect(users).toHaveLength(60);
    const types = new Set<string>(log.map((e) => (e.body.type === "domain" ? e.body.event.type : e.body.type)));
    for (const type of ["agent_text", "agent", "tool_called", "group_created", "task_created", "inbox_queued", "inbox_delivered"]) expect(types.has(type)).toBe(true);
  });

  it("gives every inbox entry its own id", () => {
    const ids = log.flatMap((e) => (e.body.type === "domain" && e.body.event.type === "inbox_queued" ? [e.body.event.entry.id] : []));
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe("markdownOf", () => {
  it("is as long as asked, with the Markdown that is costly to render", () => {
    const text = markdownOf(3, 3000);
    expect(text.length).toBeGreaterThanOrEqual(3000);
    expect(text).toMatch(/^#{2,3} /m);
    expect(text).toContain("```");
    expect(text).toContain("| --- |");
  });
});
