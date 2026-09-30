// Panel sizes of the work view: the sidebar and right column widths and the
// bottom panel height. Sizes are plain pixels; what is not stored keeps the
// CSS default (App.css), so a fresh install looks exactly like the design.
// Everything here is pure so the limits and the stored form are testable.

import type { Prefs } from "../store/prefs";

/** Preference key: the sizes the user dragged the panels to. */
export const LAYOUT_KEY = "yhtye.layout";

/** `git` and `output` are the bottom panel's height while it shows each of them. */
export type LayoutKey = "sidebar" | "right" | "git" | "output";
export type LayoutSizes = Partial<Record<LayoutKey, number>>;

export interface Bounds {
  min: number;
  max: number;
}

/** Which side of the handle the panel being resized is on. */
export type Side = "before" | "after";
export type Orientation = "vertical" | "horizontal";

/** The icon rail's fixed width (App.css `.rail`). */
export const RAIL_WIDTH = 44;
/** The conversation's and the right column's `min-width` (design §2). */
export const CONVERSATION_MIN = 380;
export const RIGHT_MIN = 420;
export const SIDEBAR_MIN = 160;
export const SIDEBAR_MAX = 420;
/** Room kept for the task list above the bottom panel, and for the panel itself. */
export const TASKS_MIN = 160;
export const BOTTOM_MIN = 120;
/** Sanity ceiling for a stored size (a corrupt value must not break the layout). */
const STORED_MAX = 4000;

/** Pixels moved by one arrow key press, and with Shift held. */
export const KEY_STEP = 16;
export const KEY_STEP_LARGE = 64;

/** `undefined` for a container that is not laid out yet (no measure, e.g. jsdom). */
const known = (size: number | undefined): size is number => size !== undefined && Number.isFinite(size) && size > 0;

/** `value` inside `min..max`; when the room is smaller than `min`, the room wins (nothing overflows). */
export function clampSize(value: number, { min, max }: Bounds): number {
  return Math.min(max, Math.max(min, value));
}

/** The sidebar may not squeeze the conversation and the right column below their minimum. */
export function sidebarBounds(bodyWidth?: number): Bounds {
  if (!known(bodyWidth)) return { min: SIDEBAR_MIN, max: SIDEBAR_MAX };
  const room = bodyWidth - RAIL_WIDTH - CONVERSATION_MIN - RIGHT_MIN;
  return { min: SIDEBAR_MIN, max: Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, room)) };
}

/** The right column leaves the conversation its minimum width. */
export function rightBounds(workspaceWidth?: number): Bounds {
  if (!known(workspaceWidth)) return { min: RIGHT_MIN, max: STORED_MAX };
  return { min: RIGHT_MIN, max: Math.max(RIGHT_MIN, workspaceWidth - CONVERSATION_MIN) };
}

/** The bottom panel leaves the task list its minimum height. */
export function bottomBounds(columnHeight?: number): Bounds {
  if (!known(columnHeight)) return { min: BOTTOM_MIN, max: STORED_MAX };
  return { min: BOTTOM_MIN, max: Math.max(BOTTOM_MIN, columnHeight - TASKS_MIN) };
}

/** The limits a stored value is held to when read back, before any container is measured. */
const STORED_BOUNDS: Record<LayoutKey, Bounds> = {
  sidebar: sidebarBounds(),
  right: rightBounds(),
  git: bottomBounds(),
  output: bottomBounds(),
};

/**
 * Size of a panel after the pointer moved `delta` px along the resize axis
 * from where the gesture started at `start` px.
 */
export function dragSize(start: number, delta: number, side: Side, bounds: Bounds): number {
  return Math.round(clampSize(side === "before" ? start + delta : start - delta, bounds));
}

/**
 * Size after a key press on the handle, or `null` for a key it does not use.
 * Arrows move the boundary in their direction; Home/End go to the smallest and
 * largest size (the ARIA window splitter's value is the panel's size).
 */
export function keyboardSize(
  key: string,
  shift: boolean,
  o: { orientation: Orientation; side: Side; current: number; bounds: Bounds },
): number | null {
  if (key === "Home") return o.bounds.min;
  if (key === "End") return o.bounds.max;
  const forward = o.orientation === "vertical" ? "ArrowRight" : "ArrowDown";
  const backward = o.orientation === "vertical" ? "ArrowLeft" : "ArrowUp";
  if (key !== forward && key !== backward) return null;
  const step = shift ? KEY_STEP_LARGE : KEY_STEP;
  const boundaryMove = key === forward ? step : -step;
  return dragSize(o.current, boundaryMove, o.side, o.bounds);
}

/** The stored sizes from their JSON text; anything unusable is dropped (the default applies). */
export function parseLayout(raw: string | null): LayoutSizes {
  if (raw === null) return {};
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return {};
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
  const stored = value as Record<string, unknown>;
  const sizes: LayoutSizes = {};
  for (const key of Object.keys(STORED_BOUNDS) as LayoutKey[]) {
    const size = stored[key];
    if (typeof size === "number" && Number.isFinite(size) && size > 0) {
      sizes[key] = Math.round(clampSize(size, STORED_BOUNDS[key]));
    }
  }
  return sizes;
}

export function loadLayout(prefs: Prefs): LayoutSizes {
  return parseLayout(prefs.get(LAYOUT_KEY));
}

/** `sizes` with one size set, or dropped (`null`: back to the default). */
export function withSize(sizes: LayoutSizes, key: LayoutKey, size: number | null): LayoutSizes {
  const { [key]: _old, ...rest } = sizes;
  return size === null ? rest : { ...rest, [key]: Math.round(clampSize(size, STORED_BOUNDS[key])) };
}

/**
 * Stores one size. The whole record is re-read first: the sidebar and the
 * workspace each hold their own copy of the sizes, and neither may overwrite
 * what the other saved.
 */
export function saveSize(prefs: Prefs, key: LayoutKey, size: number | null): LayoutSizes {
  const next = withSize(loadLayout(prefs), key, size);
  prefs.set(LAYOUT_KEY, JSON.stringify(next));
  return next;
}
