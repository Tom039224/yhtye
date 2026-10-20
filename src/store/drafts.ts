// Unsent composer text and attachments, one draft per chat. Kept in the app
// store (not in a project view, which is rebuilt when a project is opened) so a
// draft survives showing another chat or another project; it lasts as long as
// the app runs.

import type { AttachmentKind, AttachmentMeta } from "../api/generated";

/** A file the user attached to the draft: being staged, staged, or refused. */
export interface DraftAttachment {
  /** Identifies it in the draft (the core's id is known only once it is staged). */
  key: string;
  /** The file's name (the core's, once staged). */
  name: string;
  /** What it is: the core's answer once staged, guessed from the file's type until then. */
  kind: AttachmentKind;
  /** A `data:` URL of an image, for its tile (`null` for other files). */
  preview: string | null;
  status: "uploading" | "staged" | "failed";
  /** The core's record (`staged` only). */
  meta: AttachmentMeta | null;
  /** Why staging failed (`failed` only). */
  error: string | null;
}

export interface Drafts {
  /** The chat's draft text (`""` when there is none). */
  get(project: string, chat: string): string;
  /** Remembers `text` as the chat's draft; an empty text forgets it. */
  set(project: string, chat: string, text: string): void;
  /** The chat's draft attachments, in the order they were added (the same array until they change). */
  attachments(project: string, chat: string): readonly DraftAttachment[];
  /** Replaces the chat's draft attachments with `f` of them. */
  updateAttachments(project: string, chat: string, f: (list: readonly DraftAttachment[]) => readonly DraftAttachment[]): void;
  /** Forgets the chat's whole draft (the chat was deleted). */
  clear(project: string, chat: string): void;
  /** Called after any attachment change (for `useSyncExternalStore`). */
  subscribe(listener: () => void): () => void;
}

const draftKey = (project: string, chat: string): string => JSON.stringify([project, chat]);
const NO_ATTACHMENTS: readonly DraftAttachment[] = [];

export function memoryDrafts(): Drafts {
  const texts = new Map<string, string>();
  const files = new Map<string, readonly DraftAttachment[]>();
  const listeners = new Set<() => void>();
  const notify = () => {
    for (const l of [...listeners]) l();
  };
  const setFiles = (key: string, list: readonly DraftAttachment[]) => {
    if (list.length === 0) files.delete(key);
    else files.set(key, list);
  };
  return {
    get: (project, chat) => texts.get(draftKey(project, chat)) ?? "",
    set: (project, chat, text) => {
      if (text === "") texts.delete(draftKey(project, chat));
      else texts.set(draftKey(project, chat), text);
    },
    attachments: (project, chat) => files.get(draftKey(project, chat)) ?? NO_ATTACHMENTS,
    updateAttachments: (project, chat, f) => {
      const key = draftKey(project, chat);
      const before = files.get(key) ?? NO_ATTACHMENTS;
      const after = f(before);
      if (after === before) return;
      setFiles(key, after);
      notify();
    },
    clear: (project, chat) => {
      const key = draftKey(project, chat);
      texts.delete(key);
      if (!files.delete(key)) return;
      notify();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/**
 * `drafts` as one host's view of it: project ids repeat across hosts, so the
 * host's `scope` (see `AppStoreOptions.keyScope`) goes in front of them. An
 * empty scope (the local core) is `drafts` itself.
 */
export function scopedDrafts(drafts: Drafts, scope: string): Drafts {
  if (scope === "") return drafts;
  return {
    get: (project, chat) => drafts.get(scope + project, chat),
    set: (project, chat, text) => drafts.set(scope + project, chat, text),
    attachments: (project, chat) => drafts.attachments(scope + project, chat),
    updateAttachments: (project, chat, f) => drafts.updateAttachments(scope + project, chat, f),
    clear: (project, chat) => drafts.clear(scope + project, chat),
    subscribe: (listener) => drafts.subscribe(listener),
  };
}
