import { type RefObject, useLayoutEffect, useRef } from "react";

const NEAR_BOTTOM_PX = 48;

/**
 * Keeps a scroll container pinned to the bottom when `content` changes, unless
 * the user has scrolled up to read (then it stays where it is).
 */
export function useAutoScroll(ref: RefObject<HTMLElement | null>, content: unknown): void {
  const pinned = useRef(true);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onScroll = () => {
      pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_BOTTOM_PX;
    };
    el.addEventListener("scroll", onScroll);
    return () => el.removeEventListener("scroll", onScroll);
  }, [ref]);

  useLayoutEffect(() => {
    const el = ref.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [ref, content]);
}
