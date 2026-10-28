// A user message's attachments in the conversation: below its text as the
// composer's square tiles (no ×), the delivery badge the final one of the
// `prompted` event; images are read through the store and open large.

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ApiEvent, AttachmentMeta } from "../api/generated";
import { ev, ORCHESTRATOR } from "../test/events";
import { TranscriptView } from "../ui/TranscriptView";
import { applyTranscriptEvent, prependTranscripts, type Transcripts } from "./transcript";

const IMAGE: AttachmentMeta = { id: "0000000000000001", name: "shot.png", mime: "image/png", size: 2048, kind: "image", delivery: "native" };
const LOG: AttachmentMeta = { id: "0000000000000002", name: "build.log", mime: "text/plain", size: 12, kind: "text", delivery: "native" };

function userMessage(seq: number, inboxId: number, text: string, attachments?: AttachmentMeta[]): ApiEvent {
  return ev(seq, {
    type: "domain",
    event: { type: "inbox_queued", entry: { id: inboxId, chat: "C-1", item: { kind: "user_message", attrs: [], body: text, attachments } } },
  });
}

function prompted(seq: number, deliveries: { inbox_id: number; id: string; delivery: "native" | "path" }[]): ApiEvent {
  return ev(seq, { type: "prompted", session: ORCHESTRATOR, text: "batch", attachments: deliveries });
}

const fold = (events: ApiEvent[]): Transcripts => events.reduce((t, e) => applyTranscriptEvent(t, e), {} as Transcripts);

describe("folding", () => {
  it("keeps a user message's attachments, and none for a message without", () => {
    const t = fold([userMessage(1, 1, "see this", [IMAGE, LOG]), userMessage(2, 2, "plain")]);
    expect(t[ORCHESTRATOR]).toMatchObject([
      { kind: "user", text: "see this", attachments: [IMAGE, LOG] },
      { kind: "user", text: "plain", attachments: [] },
    ]);
  });

  it("puts the final delivery of `prompted` on the message's attachments", () => {
    const t = fold([userMessage(1, 1, "", [IMAGE, LOG]), prompted(2, [{ inbox_id: 1, id: LOG.id, delivery: "path" }, { inbox_id: 1, id: IMAGE.id, delivery: "native" }])]);
    expect(t[ORCHESTRATOR]).toHaveLength(1);
    const [item] = t[ORCHESTRATOR];
    expect(item.kind === "user" && item.attachments.map((a) => a.delivery)).toEqual(["native", "path"]);
  });

  it("applies a delivery whose message is in an older page once that page is loaded", () => {
    const newer = fold([prompted(5, [{ inbox_id: 1, id: LOG.id, delivery: "path" }])]);
    expect(newer[ORCHESTRATOR]).toMatchObject([{ kind: "delivered" }]);
    const older = fold([userMessage(1, 1, "log", [LOG])]);
    const merged = prependTranscripts(older, newer)[ORCHESTRATOR];
    const user = merged.find((i) => i.kind === "user");
    expect(user?.kind === "user" && user.attachments[0].delivery).toBe("path");
  });

  it("does not add anything for a prompt without attachments", () => {
    const t = fold([userMessage(1, 1, "hi"), ev(2, { type: "prompted", session: ORCHESTRATOR, text: "batch" })]);
    expect(t[ORCHESTRATOR]).toHaveLength(1);
  });
});

describe("TranscriptView", () => {
  it("shows the attachments below the text, with how each was delivered and no ×", async () => {
    const t = fold([userMessage(1, 1, "see this", [IMAGE, LOG]), prompted(2, [{ inbox_id: 1, id: LOG.id, delivery: "path" }])]);
    const load = vi.fn().mockResolvedValue("data:image/png;base64,AAAA");
    render(<TranscriptView items={t[ORCHESTRATOR]} agentLabel="orchestrator" loadAttachment={load} />);
    const list = screen.getByRole("list", { name: "添付ファイル" });
    const text = screen.getByText("see this");
    // The text comes first, the tiles after it.
    expect(text.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const tiles = within(list).getAllByRole("listitem");
    expect(tiles).toHaveLength(2);
    expect(within(tiles[0]).getByRole("img", { name: /モデルに直接渡す/ })).toBeInTheDocument();
    expect(within(tiles[1]).getByRole("img", { name: /パスで渡す/ })).toBeInTheDocument();
    expect(within(tiles[1]).getByText("LOG")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /を外す/ })).not.toBeInTheDocument();
    expect(await within(tiles[0]).findByRole("img", { name: "shot.png" })).toHaveAttribute("src", "data:image/png;base64,AAAA");
    // Only images are read.
    expect(load).toHaveBeenCalledTimes(1);
    expect(load).toHaveBeenCalledWith(IMAGE.id);
  });

  it("opens an image large on click and closes it with Escape", async () => {
    const user = userEvent.setup();
    const t = fold([userMessage(1, 1, "", [IMAGE])]);
    render(<TranscriptView items={t[ORCHESTRATOR]} agentLabel="orchestrator" loadAttachment={() => Promise.resolve("data:image/png;base64,AAAA")} />);
    await user.click(await screen.findByRole("button", { name: "shot.png を拡大表示" }));
    const dialog = screen.getByRole("dialog", { name: "shot.png" });
    expect(within(dialog).getByRole("img", { name: "shot.png" })).toHaveAttribute("src", "data:image/png;base64,AAAA");
    expect(within(dialog).getByRole("button", { name: "閉じる" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("shows a message of attachments only without an empty bubble, and still marks it queued", () => {
    const t = fold([userMessage(1, 1, "", [LOG])]);
    const { container } = render(<TranscriptView items={t[ORCHESTRATOR]} agentLabel="orchestrator" queued={new Set([1])} />);
    expect(container.querySelector(".msg-user .msg-body")).toBeNull();
    expect(screen.getByText("待機中")).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
  });

  it("keeps the image icon when its preview cannot be read, saying why in the tooltip", async () => {
    const t = fold([userMessage(1, 1, "x", [IMAGE])]);
    render(<TranscriptView items={t[ORCHESTRATOR]} agentLabel="orchestrator" loadAttachment={() => Promise.reject(new Error("offline"))} />);
    const tile = screen.getByRole("listitem");
    await vi.waitFor(() => expect(tile.getAttribute("title")).toContain("プレビューを読めませんでした: offline"));
    expect(within(tile).queryByRole("img", { name: "shot.png" })).not.toBeInTheDocument();
    expect(within(tile).queryByRole("button")).not.toBeInTheDocument();
  });
});
