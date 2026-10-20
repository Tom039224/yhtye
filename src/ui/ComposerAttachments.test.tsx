// The composer's attachments: the + button (a file picker), pasting an image,
// dropping files; square tiles above the text with the delivery badge and a ×;
// attachments alone can be sent once all are staged.

import { createEvent, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { DraftAttachment } from "../store/drafts";
import { Composer } from "./Composer";

const staged = (key: string, name: string, delivery: "native" | "path", preview: string | null = null): DraftAttachment => ({
  key,
  name,
  kind: preview ? "image" : "file",
  preview,
  status: "staged",
  meta: { id: `id-${key}`, name, mime: preview ? "image/png" : "application/pdf", size: 4, kind: preview ? "image" : "file", delivery },
  error: null,
});

function setup(props: { attachments?: DraftAttachment[]; disabled?: boolean } = {}) {
  const onSend = vi.fn().mockResolvedValue(true);
  const onAddFiles = vi.fn();
  const onRemoveAttachment = vi.fn();
  render(
    <Composer
      disabled={props.disabled ?? false}
      disabledReason={null}
      sending={false}
      turnRunning={false}
      mentions={[]}
      onRemoveMention={() => {}}
      attachments={props.attachments ?? []}
      onAddFiles={onAddFiles}
      onRemoveAttachment={onRemoveAttachment}
      onSend={onSend}
      onCancel={() => {}}
    />,
  );
  return { onSend, onAddFiles, onRemoveAttachment, user: userEvent.setup() };
}

const textbox = () => screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" });
const png = () => new File([new Uint8Array([1, 2])], "image.png", { type: "image/png" });

describe("adding attachments", () => {
  it("has a + button left of the text that opens a multi-file picker", async () => {
    const { onAddFiles, user } = setup();
    const plus = screen.getByRole("button", { name: "ファイルを添付" });
    expect(plus.compareDocumentPosition(textbox()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const input = screen.getByTestId("attach-input") as HTMLInputElement;
    expect(input).toHaveAttribute("type", "file");
    expect(input.multiple).toBe(true);
    const click = vi.spyOn(input, "click");
    await user.click(plus);
    expect(click).toHaveBeenCalled();
    const files = [new File(["a"], "a.pdf", { type: "application/pdf" }), new File(["b"], "b.log")];
    await user.upload(input, files);
    expect(onAddFiles).toHaveBeenCalledWith(files);
    // Cleared so that the same file can be picked again.
    expect(input.value).toBe("");
  });

  it("attaches an image pasted from the clipboard", () => {
    const { onAddFiles } = setup();
    const file = png();
    const event = createEvent.paste(textbox(), { clipboardData: { files: [file], getData: () => "" } });
    fireEvent(textbox(), event);
    expect(onAddFiles).toHaveBeenCalledWith([file]);
    expect(event.defaultPrevented).toBe(true);
  });

  it("pastes text normally, also when the copy has an image rendering of it", () => {
    const { onAddFiles } = setup();
    const event = createEvent.paste(textbox(), { clipboardData: { files: [png()], getData: (t: string) => (t === "text/plain" ? "a\tb" : "") } });
    fireEvent(textbox(), event);
    expect(onAddFiles).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
  });

  it("attaches dropped files and highlights the composer while they are dragged over it", () => {
    const { onAddFiles } = setup();
    const composer = document.querySelector(".composer") as HTMLElement;
    const file = png();
    const dataTransfer = { types: ["Files"], files: [file], dropEffect: "none" };
    fireEvent.dragOver(composer, { dataTransfer });
    expect(composer).toHaveClass("composer-dragging");
    fireEvent.drop(composer, { dataTransfer });
    expect(composer).not.toHaveClass("composer-dragging");
    expect(onAddFiles).toHaveBeenCalledWith([file]);
  });

  it("takes no files while it is disabled", () => {
    const { onAddFiles } = setup({ disabled: true });
    expect(screen.getByRole("button", { name: "ファイルを添付" })).toBeDisabled();
    fireEvent.drop(document.querySelector(".composer") as HTMLElement, { dataTransfer: { types: ["Files"], files: [png()] } });
    expect(onAddFiles).not.toHaveBeenCalled();
  });
});

describe("attachment tiles", () => {
  it("shows them above the text: a thumbnail or the file's type, the delivery at the upper left, a × to remove", async () => {
    const { onRemoveAttachment, user } = setup({
      attachments: [staged("a", "shot.png", "native", "data:image/png;base64,AAAA"), staged("b", "report.pdf", "path")],
    });
    const list = screen.getByRole("list", { name: "添付ファイル" });
    expect(list.compareDocumentPosition(textbox()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const [image, pdf] = within(list).getAllByRole("listitem");
    expect(within(image).getByRole("img", { name: "shot.png" })).toHaveAttribute("src", "data:image/png;base64,AAAA");
    expect(within(image).getByRole("img", { name: /モデルに直接渡す \(native\) · 送信時に確定/ })).toBeInTheDocument();
    expect(within(pdf).getByText("PDF")).toBeInTheDocument();
    expect(within(pdf).getByRole("img", { name: /ファイルとして保存し、パスで渡す \(path\)/ })).toBeInTheDocument();
    await user.click(within(pdf).getByRole("button", { name: "report.pdf を外す" }));
    expect(onRemoveAttachment).toHaveBeenCalledWith("b");
  });

  it("shows a file being uploaded and one that failed, and does not send until they are dealt with", () => {
    const uploading: DraftAttachment = { key: "u", name: "big.zip", kind: "file", preview: null, status: "uploading", meta: null, error: null };
    const failed: DraftAttachment = { ...uploading, key: "f", name: "bad.bin", status: "failed", error: "disk full" };
    setup({ attachments: [uploading, failed] });
    expect(screen.getByRole("status", { name: "big.zip を添付中" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "bad.bin を添付できませんでした" })).toBeInTheDocument();
    const [, failedTile] = screen.getAllByRole("listitem");
    expect(failedTile.getAttribute("title")).toContain("disk full");
    // No delivery badge until staged.
    expect(screen.queryByRole("img", { name: /native|path/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "送信" })).toBeDisabled();
    expect(screen.getByText(/添付中…/)).toBeInTheDocument();
  });

  it("sends attachments without text", async () => {
    const { onSend, user } = setup({ attachments: [staged("b", "report.pdf", "path")] });
    const send = screen.getByRole("button", { name: "送信" });
    expect(send).toBeEnabled();
    await user.click(send);
    expect(onSend).toHaveBeenCalledWith("");
  });

  it("opens a staged image large", async () => {
    const { user } = setup({ attachments: [staged("a", "shot.png", "native", "data:image/png;base64,AAAA")] });
    await user.click(screen.getByRole("button", { name: "shot.png を拡大表示" }));
    expect(screen.getByRole("dialog", { name: "shot.png" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "閉じる" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
