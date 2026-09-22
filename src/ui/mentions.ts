// Task references the user puts into the composer with a task card's `@`
// button (design §4). The orchestrator knows tasks by id, so the message
// starts with one `@T-n title` line per reference.

export interface Mention {
  task: string;
  title: string;
}

/** Adds `m` unless the task is already referenced. */
export function addMention(list: Mention[], m: Mention): Mention[] {
  return list.some((x) => x.task === m.task) ? list : [...list, m];
}

export function removeMention(list: Mention[], task: string): Mention[] {
  return list.filter((x) => x.task !== task);
}

/** The text sent to the orchestrator: reference lines, a blank line, the message. */
export function withMentions(text: string, mentions: Mention[]): string {
  if (mentions.length === 0) return text;
  const refs = mentions.map((m) => `@${m.task} ${m.title}`).join("\n");
  return `${refs}\n\n${text}`;
}
