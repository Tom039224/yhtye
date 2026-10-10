import { type HTMLAttributes, type ReactNode, useEffect, useLayoutEffect, useRef } from "react";

/** What one wheel "line" (`WheelEvent.deltaMode` 1) moves a strip, in px. */
const WHEEL_LINE_PX = 16;

interface Props extends HTMLAttributes<HTMLDivElement> {
  /** Identifies the shown item: when it changes, the item marked `.active` is brought into view. */
  active: unknown;
  children: ReactNode;
}

/**
 * A single row of items that scrolls sideways (no scrollbar) instead of squeezing or pushing out its
 * neighbours: the vertical wheel scrolls it too, and the item marked `.active` is kept in view
 * whenever `active` changes. The row's look is the caller's (App.css `.scroll-strip` only makes it scroll).
 */
export function ScrollStrip({ active, className, children, ...rest }: Props) {
  const strip = useRef<HTMLDivElement>(null);

  // A native listener: React's `onWheel` is passive, which cannot keep the page from also scrolling.
  useEffect(() => {
    const el = strip.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      // Nothing to scroll, or the gesture is already sideways (a trackpad, Shift + wheel).
      if (el.scrollWidth <= el.clientWidth || Math.abs(e.deltaX) >= Math.abs(e.deltaY)) return;
      const unit = e.deltaMode === 1 ? WHEEL_LINE_PX : e.deltaMode === 2 ? el.clientWidth : 1;
      el.scrollLeft += e.deltaY * unit;
      e.preventDefault();
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  useLayoutEffect(() => {
    // "nearest" on both axes: moves the strip just far enough, and nothing else when the item is in view.
    strip.current?.querySelector<HTMLElement>(".active")?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [active]);

  return (
    <div ref={strip} className={`scroll-strip ${className ?? ""}`} {...rest}>
      {children}
    </div>
  );
}
