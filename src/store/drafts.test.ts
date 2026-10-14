import { describe, expect, it } from "vitest";

import { memoryDrafts, scopedDrafts } from "./drafts";

describe("memoryDrafts", () => {
  it("is empty for a chat that has no draft", () => {
    expect(memoryDrafts().get("repo", "C-1")).toBe("");
  });

  it("keeps a draft per chat", () => {
    const drafts = memoryDrafts();
    drafts.set("repo", "C-1", "first");
    drafts.set("repo", "C-2", "second");
    expect(drafts.get("repo", "C-1")).toBe("first");
    expect(drafts.get("repo", "C-2")).toBe("second");
  });

  it("keeps the same chat id of two projects apart", () => {
    const drafts = memoryDrafts();
    drafts.set("repo", "C-1", "in repo");
    drafts.set("other", "C-1", "in other");
    expect(drafts.get("repo", "C-1")).toBe("in repo");
    expect(drafts.get("other", "C-1")).toBe("in other");
  });

  it("does not mix up ids that only join into the same string", () => {
    const drafts = memoryDrafts();
    drafts.set("a/b", "c", "x");
    expect(drafts.get("a", "b/c")).toBe("");
  });

  it("replaces a draft and forgets it when set to an empty text", () => {
    const drafts = memoryDrafts();
    drafts.set("repo", "C-1", "old");
    drafts.set("repo", "C-1", "new");
    expect(drafts.get("repo", "C-1")).toBe("new");
    drafts.set("repo", "C-1", "");
    expect(drafts.get("repo", "C-1")).toBe("");
  });

  it("does not share drafts between stores", () => {
    const a = memoryDrafts();
    a.set("repo", "C-1", "mine");
    expect(memoryDrafts().get("repo", "C-1")).toBe("");
  });
});

describe("scopedDrafts", () => {
  it("is the drafts themselves for the local core (empty scope)", () => {
    const drafts = memoryDrafts();
    expect(scopedDrafts(drafts, "")).toBe(drafts);
  });

  it("keeps the same project id of two hosts apart", () => {
    const drafts = memoryDrafts();
    const local = scopedDrafts(drafts, "");
    const remote = scopedDrafts(drafts, "host:sid-1:");
    const other = scopedDrafts(drafts, "host:sid-2:");
    local.set("repo", "C-1", "on this PC");
    remote.set("repo", "C-1", "on the server");
    expect(local.get("repo", "C-1")).toBe("on this PC");
    expect(remote.get("repo", "C-1")).toBe("on the server");
    expect(other.get("repo", "C-1")).toBe("");
    remote.set("repo", "C-1", "");
    expect(remote.get("repo", "C-1")).toBe("");
    expect(local.get("repo", "C-1")).toBe("on this PC");
  });
});
