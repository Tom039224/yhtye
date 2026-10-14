// Unsent composer text, one per chat. Kept in the app store (not in a project
// view, which is rebuilt when a project is opened) so a draft survives showing
// another chat or another project; it lasts as long as the app runs.

export interface Drafts {
  /** The chat's draft (`""` when there is none). */
  get(project: string, chat: string): string;
  /** Remembers `text` as the chat's draft; an empty text forgets it. */
  set(project: string, chat: string, text: string): void;
}

const draftKey = (project: string, chat: string): string => JSON.stringify([project, chat]);

export function memoryDrafts(): Drafts {
  const drafts = new Map<string, string>();
  return {
    get: (project, chat) => drafts.get(draftKey(project, chat)) ?? "",
    set: (project, chat, text) => {
      if (text === "") drafts.delete(draftKey(project, chat));
      else drafts.set(draftKey(project, chat), text);
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
  };
}
