import { type RefObject, useLayoutEffect, useRef } from "react";

const NEAR_BOTTOM_PX = 48;

/**
 * Keeps a scroll container pinned to the bottom when `content` changes, unless
 * the user has scrolled up to read (then it stays where it is). When `head`
 * (the first item's identity) changes, older content was prepended: the view
 * keeps its place instead of jumping.
 */
export function useAutoScroll(ref: RefObject<HTMLElement | null>, content: unknown, head?: unknown): void {
  const pinned = useRef(true);
  const lastHead = useRef(head);
  const lastHeight = useRef(0);

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
    if (!el) return;
    if (head !== lastHead.current && !pinned.current) {
      el.scrollTop += el.scrollHeight - lastHeight.current;
    } else if (pinned.current) {
      el.scrollTop = el.scrollHeight;
    }
    lastHead.current = head;
    lastHeight.current = el.scrollHeight;
  }, [ref, content, head]);
}
