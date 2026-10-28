// Attachments of the composer (core-design.md §20): a file is staged with the
// core as soon as it is picked, pasted or dropped, shown as a tile of the
// chat's draft, and sent with the message by id. Sent images are read back with
// `get_attachment` for the transcript (cached).

import type { AttachmentKind, AttachmentMeta } from "../api/generated";
import { type Transport, toCommandError } from "../api/transport";
import type { DraftAttachment, Drafts } from "./drafts";

/** Previews of sent attachments kept in memory (the oldest go first). */
export const PREVIEW_CACHE_SIZE = 64;
/** The name given to a file that has none (an image pasted from some apps). */
const UNNAMED = "attachment";

/** What a file probably is before the core says (its content decides). */
export function guessKind(mime: string): AttachmentKind {
  if (mime.startsWith("image/")) return "image";
  if (mime.startsWith("text/") || mime === "application/json") return "text";
  return "file";
}

/** A `data:` URL that shows an attachment's content (the CSP allows `data:` images, not `blob:`). */
export function dataUrl(mime: string, base64: string): string {
  return `data:${mime};base64,${base64}`;
}

/** The file's content as base64. */
export function readBase64(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const url = typeof reader.result === "string" ? reader.result : "";
      resolve(url.slice(url.indexOf(",") + 1));
    };
    reader.onerror = () => reject(reader.error ?? new Error("cannot read the file"));
    reader.readAsDataURL(file);
  });
}

/** The ids to send with the message: the staged attachments, in order. */
export function stagedIds(list: readonly DraftAttachment[]): string[] {
  return list.flatMap((a) => (a.status === "staged" && a.meta ? [a.meta.id] : []));
}

/** Whether the draft's attachments allow sending: none is still uploading or failed. */
export function attachmentsReady(list: readonly DraftAttachment[]): boolean {
  return list.every((a) => a.status === "staged");
}

const previewKey = (project: string, chat: string, id: string): string => JSON.stringify([project, chat, id]);

export class Attachments {
  private nextKey = 1;
  private readonly previews = new Map<string, Promise<string>>();

  constructor(
    private readonly transport: Transport,
    private readonly drafts: Drafts,
    private readonly onError: (message: string) => void,
  ) {}

  /** Adds the files to the chat's draft and stages each with the core. */
  async attach(project: string, chat: string, files: readonly File[]): Promise<void> {
    await Promise.all(files.map((file) => this.attachOne(project, chat, file)));
  }

  /**
   * Takes an attachment out of the draft; a staged one is discarded by the
   * core (one still uploading is discarded when its staging answers).
   */
  async remove(project: string, chat: string, key: string): Promise<void> {
    const item = this.drafts.attachments(project, chat).find((a) => a.key === key);
    if (!item) return;
    this.drafts.updateAttachments(project, chat, (list) => list.filter((a) => a.key !== key));
    if (item.meta) await this.discard(project, chat, item.meta.id);
  }

  /** The attachments were sent: they leave the draft, and their previews are kept for the transcript. */
  sent(project: string, chat: string, sent: readonly DraftAttachment[]): void {
    for (const a of sent) {
      if (a.meta && a.preview) this.remember(previewKey(project, chat, a.meta.id), Promise.resolve(a.preview));
    }
    const keys = new Set(sent.map((a) => a.key));
    this.drafts.updateAttachments(project, chat, (list) => list.filter((a) => !keys.has(a.key)));
  }

  /** A sent attachment's content as a `data:` URL (read once, then cached). Rejects with a CommandError. */
  preview(project: string, chat: string, id: string): Promise<string> {
    const key = previewKey(project, chat, id);
    const cached = this.previews.get(key);
    if (cached) {
      this.remember(key, cached);
      return cached;
    }
    const read = this.transport.invoke({ type: "get_attachment", project, chat, id }).then((r) => {
      if (r.type !== "attachment") throw new Error(`unexpected response: ${r.type}`);
      return dataUrl(r.attachment.mime, r.data_base64);
    });
    this.remember(key, read);
    // A failure is not cached: the next tile that needs it asks again.
    read.catch(() => {
      if (this.previews.get(key) === read) this.previews.delete(key);
    });
    return read;
  }

  private remember(key: string, preview: Promise<string>): void {
    this.previews.delete(key);
    this.previews.set(key, preview);
    while (this.previews.size > PREVIEW_CACHE_SIZE) {
      const oldest = this.previews.keys().next().value;
      if (oldest === undefined) break;
      this.previews.delete(oldest);
    }
  }

  private async attachOne(project: string, chat: string, file: File): Promise<void> {
    const key = `attachment-${this.nextKey++}`;
    const item: DraftAttachment = {
      key,
      name: file.name || UNNAMED,
      kind: guessKind(file.type),
      preview: null,
      status: "uploading",
      meta: null,
      error: null,
    };
    this.drafts.updateAttachments(project, chat, (list) => [...list, item]);
    const update = (f: (a: DraftAttachment) => DraftAttachment) =>
      this.drafts.updateAttachments(project, chat, (list) => (list.some((a) => a.key === key) ? list.map((a) => (a.key === key ? f(a) : a)) : list));
    try {
      const base64 = await readBase64(file);
      const r = await this.transport.invoke({
        type: "stage_attachment",
        project,
        chat,
        name: item.name,
        mime: file.type || undefined,
        data_base64: base64,
      });
      if (r.type !== "attachment_staged") throw new Error(`unexpected response: ${r.type}`);
      const meta = r.attachment;
      if (!this.drafts.attachments(project, chat).some((a) => a.key === key)) {
        // Removed while it was uploading.
        await this.discard(project, chat, meta.id);
        return;
      }
      update((a) => ({ ...a, ...stagedFields(meta, base64) }));
    } catch (e) {
      update((a) => ({ ...a, status: "failed", error: toCommandError(e).message }));
    }
  }

  private async discard(project: string, chat: string, id: string): Promise<void> {
    try {
      await this.transport.invoke({ type: "discard_attachment", project, chat, id });
    } catch (e) {
      const err = toCommandError(e);
      // Already gone (the chat was deleted): nothing is left behind.
      if (err.code !== "not_found") this.onError(`discard_attachment failed: ${err.message}`);
    }
  }
}

function stagedFields(meta: AttachmentMeta, base64: string): Partial<DraftAttachment> {
  return {
    status: "staged",
    meta,
    name: meta.name,
    kind: meta.kind,
    preview: meta.kind === "image" ? dataUrl(meta.mime, base64) : null,
    error: null,
  };
}
