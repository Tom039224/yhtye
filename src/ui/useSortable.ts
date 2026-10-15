// Drag-and-drop reordering of one list of rows, with the browser's own drag
// events (no library): a row is dragged by its handle props and dropped onto
// another row of the same list, where it takes that row's place. The same
// reorder is offered without the mouse by `moveActions` (a row menu) and
// `reorderKey` (Alt + ↑ / ↓).

import { type DragEvent, type HTMLAttributes, type KeyboardEvent, useState } from "react";

import { dropOnto, step } from "../store/order";
import type { RowMenuAction } from "./RowMenu";

export type DropMark = "above" | "below";

type RowProps = Pick<HTMLAttributes<HTMLElement>, "onDragStart" | "onDragOver" | "onDragLeave" | "onDrop" | "onDragEnd"> & {
  draggable: boolean;
  "data-dragging"?: true;
  /** Where a drop would put the dragged row, relative to this one. */
  "data-drop"?: DropMark;
};

export interface Sortable {
  /** Spread on a row: makes it draggable and a place to drop on. */
  rowProps: (key: string) => RowProps;
}

/**
 * `keys` is the order of the rows on screen; `onMove` gets the whole list as
 * the drop makes it. Rows of another list (or a drag from elsewhere) are ignored.
 */
export function useSortable(keys: readonly string[], onMove: (next: string[]) => void, enabled = true): Sortable {
  const [dragging, setDragging] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);
  const reset = () => {
    setDragging(null);
    setOver(null);
  };
  const markOf = (key: string): DropMark | undefined => {
    if (dragging === null || over !== key || dragging === key) return undefined;
    return keys.indexOf(dragging) < keys.indexOf(key) ? "below" : "above";
  };
  const rowProps = (key: string): RowProps => ({
    draggable: enabled,
    "data-dragging": dragging === key ? true : undefined,
    "data-drop": markOf(key),
    onDragStart: (e: DragEvent<HTMLElement>) => {
      e.stopPropagation();
      e.dataTransfer.effectAllowed = "move";
      // Some engines start no drag without data.
      e.dataTransfer.setData("text/plain", key);
      setDragging(key);
    },
    onDragOver: (e: DragEvent<HTMLElement>) => {
      if (dragging === null || dragging === key) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      if (over !== key) setOver(key);
    },
    onDragLeave: (e: DragEvent<HTMLElement>) => {
      if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
      if (over === key) setOver(null);
    },
    onDrop: (e: DragEvent<HTMLElement>) => {
      if (dragging === null) return;
      e.preventDefault();
      e.stopPropagation();
      const next = dropOnto(keys, dragging, key);
      reset();
      if (next.some((k, i) => k !== keys[i])) onMove(next);
    },
    onDragEnd: reset,
  });
  return { rowProps };
}

/** The "move up" / "move down" entries of a row's menu (disabled at the ends of the list). */
export function moveActions(keys: readonly string[], key: string, onMove: (next: string[]) => void): RowMenuAction[] {
  const up = step(keys, key, -1);
  const down = step(keys, key, 1);
  return [
    { id: "move-up", label: "上へ移動", icon: "arrow-up", disabled: !up, onSelect: () => up && onMove(up) },
    { id: "move-down", label: "下へ移動", icon: "arrow-down", disabled: !down, onSelect: () => down && onMove(down) },
  ];
}

/**
 * Alt + ↑ / ↓ on the row of `key` moves it one place; true when the key was
 * taken (the caller then stops handling it).
 */
export function reorderKey(e: KeyboardEvent, keys: readonly string[], key: string, onMove: (next: string[]) => void): boolean {
  if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return false;
  e.preventDefault();
  const next = step(keys, key, e.key === "ArrowUp" ? -1 : 1);
  if (next) onMove(next);
  return true;
}
