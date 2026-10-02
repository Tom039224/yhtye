import { type KeyboardEvent, type PointerEvent, useEffect, useRef, useState } from "react";

import { type Bounds, dragSize, keyboardSize, type Orientation, type Side } from "./layout";

interface Props {
  /** "vertical": a vertical line between side-by-side panels (resizes a width); "horizontal": between stacked panels. */
  orientation: Orientation;
  /** Which side of the handle the panel being resized is on. */
  side: Side;
  label: string;
  /** The panel's current size in px (0 when it is not laid out); read when a gesture starts. */
  measure: () => number;
  /** The limits, read when a gesture starts: the window may have changed since the last render. */
  bounds: () => Bounds;
  /** Every size the handle passes through; the parent sets it on the panel without saving it. */
  onResize: (size: number) => void;
  /** The size a gesture ends on; `null` after a reset to the default. */
  onCommit: (size: number | null) => void;
}

interface Gesture {
  pointer: number;
  origin: number;
  start: number;
  bounds: Bounds;
  size: number;
  moved: boolean;
}

interface Reading {
  now: number;
  min: number;
  max: number;
}

const BODY_CLASS = { vertical: "resizing-col", horizontal: "resizing-row" } as const;

/**
 * A drag handle on the boundary between two panels: pointer drag, arrow keys
 * (Home/End for the limits), double click or Enter to go back to the default.
 * It is a zero-width flex child that overlays the boundary, so it never
 * changes the layout itself.
 */
export function Resizer({ orientation, side, label, measure, bounds, onResize, onCommit }: Props) {
  const gesture = useRef<Gesture | null>(null);
  const [dragging, setDragging] = useState(false);
  const [reading, setReading] = useState<Reading | null>(null);
  // Bumped when a size settles, so the reading is taken after the panel has its new size.
  const [settled, setSettled] = useState(0);
  const vertical = orientation === "vertical";

  const read = (size?: number) => {
    const { min, max } = bounds();
    const now = Math.round(size ?? measure());
    setReading((prev) => (prev?.now === now && prev.min === min && prev.max === max ? prev : { now, min, max }));
  };
  // Only the first paint and a settled size need a fresh reading.
  useEffect(() => read(), [settled]);
  useEffect(() => () => document.body.classList.remove(BODY_CLASS[orientation]), [orientation]);

  const finish = (e: PointerEvent<HTMLDivElement>) => {
    const g = gesture.current;
    if (!g || g.pointer !== e.pointerId) return;
    gesture.current = null;
    setDragging(false);
    document.body.classList.remove(BODY_CLASS[orientation]);
    // A gesture that moved commits even when it came back to where it began:
    // the panel already carries that size, so the parent has to know it.
    if (g.moved) {
      onCommit(g.size);
      setSettled((n) => n + 1);
    }
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    const start = Math.round(measure());
    gesture.current = { pointer: e.pointerId, origin: vertical ? e.clientX : e.clientY, start, bounds: bounds(), size: start, moved: false };
    setDragging(true);
    document.body.classList.add(BODY_CLASS[orientation]);
  };

  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const g = gesture.current;
    if (!g || g.pointer !== e.pointerId) return;
    const size = dragSize(g.start, (vertical ? e.clientX : e.clientY) - g.origin, side, g.bounds);
    if (size === g.size) return;
    g.size = size;
    g.moved = true;
    onResize(size);
    read(size);
  };

  const reset = () => {
    onCommit(null);
    setSettled((n) => n + 1);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      reset();
      return;
    }
    const range = bounds();
    const current = Math.round(measure());
    const size = keyboardSize(e.key, e.shiftKey, { orientation, side, current, bounds: range });
    if (size === null) return;
    e.preventDefault();
    if (size === current) return;
    onResize(size);
    onCommit(size);
    setSettled((n) => n + 1);
  };

  return (
    <div
      role="separator"
      aria-orientation={orientation}
      aria-label={label}
      aria-valuenow={reading && reading.now > 0 ? reading.now : undefined}
      aria-valuemin={reading?.min}
      aria-valuemax={reading?.max}
      tabIndex={0}
      title="ドラッグで大きさを変える · ダブルクリックで元に戻す"
      className={`resizer resizer-${orientation}${dragging ? " dragging" : ""}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={finish}
      onPointerCancel={finish}
      onDoubleClick={reset}
      onKeyDown={onKeyDown}
      onFocus={() => read()}
    />
  );
}
