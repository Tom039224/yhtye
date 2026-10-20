import { describe, expect, it, vi } from "vitest";

import { type DraftAttachment, memoryDrafts } from "./drafts";

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

describe("draft attachments", () => {
  const item = (key: string): DraftAttachment => ({ key, name: `${key}.txt`, kind: "text", preview: null, status: "uploading", meta: null, error: null });

  it("keeps attachments per chat and tells subscribers when they change", () => {
    const drafts = memoryDrafts();
    const seen = vi.fn();
    const unsubscribe = drafts.subscribe(seen);
    expect(drafts.attachments("repo", "C-1")).toEqual([]);
    drafts.updateAttachments("repo", "C-1", (list) => [...list, item("a")]);
    drafts.updateAttachments("repo", "C-1", (list) => [...list, item("b")]);
    expect(drafts.attachments("repo", "C-1").map((a) => a.key)).toEqual(["a", "b"]);
    expect(drafts.attachments("repo", "C-2")).toEqual([]);
    expect(seen).toHaveBeenCalledTimes(2);
    unsubscribe();
    drafts.updateAttachments("repo", "C-1", () => []);
    expect(seen).toHaveBeenCalledTimes(2);
  });

  it("returns the same array until it changes, and does not notify for no change", () => {
    const drafts = memoryDrafts();
    const seen = vi.fn();
    drafts.subscribe(seen);
    drafts.updateAttachments("repo", "C-1", (list) => [...list, item("a")]);
    const first = drafts.attachments("repo", "C-1");
    drafts.updateAttachments("repo", "C-1", (list) => list);
    expect(drafts.attachments("repo", "C-1")).toBe(first);
    expect(seen).toHaveBeenCalledTimes(1);
  });

  it("forgets a chat's text and attachments together", () => {
    const drafts = memoryDrafts();
    drafts.set("repo", "C-1", "text");
    drafts.updateAttachments("repo", "C-1", () => [item("a")]);
    drafts.clear("repo", "C-1");
    expect(drafts.get("repo", "C-1")).toBe("");
    expect(drafts.attachments("repo", "C-1")).toEqual([]);
  });
});
