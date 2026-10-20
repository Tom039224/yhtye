// The recently-used order of the projects and of a project's branches. The core
// keeps the lists (most recently used first; `crates/yhtye-core/src/order.rs`)
// and sorts the project listing itself; the branch tree sorts its own nodes
// with the list it reads. These pure helpers do that sort and make the lists a
// drag or a menu entry asks for.

/** `items` ordered by where their key is in `order`; unlisted ones follow in the order given. */
export function sortByOrder<T>(items: readonly T[], order: readonly string[], keyOf: (item: T) => string): T[] {
  const rank = new Map<string, number>();
  order.forEach((key, i) => {
    if (!rank.has(key)) rank.set(key, i);
  });
  const at = (item: T) => rank.get(keyOf(item)) ?? Number.MAX_SAFE_INTEGER;
  // `sort` is stable: the unlisted ones keep their order.
  return [...items].sort((a, b) => at(a) - at(b));
}

/** `keys` with `key` moved to the front (added when missing). */
export function toFront(keys: readonly string[], key: string): string[] {
  return [key, ...keys.filter((k) => k !== key)];
}

/**
 * `keys` after `key` is dropped onto `target`: it takes `target`'s place, and the
 * ones in between shift by one (as a sortable list does). `keys` unchanged when
 * either is missing or they are the same.
 */
export function dropOnto(keys: readonly string[], key: string, target: string): string[] {
  const from = keys.indexOf(key);
  const to = keys.indexOf(target);
  if (from < 0 || to < 0 || from === to) return [...keys];
  const next = keys.filter((k) => k !== key);
  next.splice(to, 0, key);
  return next;
}

/** `keys` with `key` moved one place up (`-1`) or down (`1`); `null` when it cannot move that way. */
export function step(keys: readonly string[], key: string, delta: -1 | 1): string[] | null {
  const from = keys.indexOf(key);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= keys.length) return null;
  const next = [...keys];
  [next[from], next[to]] = [next[to], next[from]];
  return next;
}

/**
 * The whole list after `shown` (some of its keys, in the order the user made)
 * is put back: those keys keep the places they had in `full`, in the new order,
 * and the others do not move. Used where only some of the items are on screen.
 */
export function fillSlots(full: readonly string[], shown: readonly string[]): string[] {
  const among = new Set(shown);
  const queue = [...shown];
  return full.map((key) => (among.has(key) ? (queue.shift() ?? key) : key));
}
