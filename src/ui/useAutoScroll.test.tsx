import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { useAutoScroll } from "./useAutoScroll";

/** An element with a scroll height that can change (jsdom has no layout). */
function scroller(height: number, top: number) {
  const el = document.createElement("div");
  let scrollHeight = height;
  Object.defineProperty(el, "scrollHeight", { get: () => scrollHeight, configurable: true });
  Object.defineProperty(el, "clientHeight", { value: 100, configurable: true });
  el.scrollTop = top;
  return Object.assign(el, { setHeight: (h: number) => void (scrollHeight = h) });
}

describe("useAutoScroll", () => {
  it("starts at the bottom of another chat after the reader scrolled up in the first", () => {
    const el = scroller(1000, 0);
    const ref = { current: el };
    const { rerender } = renderHook(({ chat }) => useAutoScroll(ref, [chat], 1, chat), { initialProps: { chat: "C-1" } });
    // The reader scrolls up in C-1 (far from the bottom).
    el.scrollTop = 100;
    el.dispatchEvent(new Event("scroll"));
    rerender({ chat: "C-2" });
    expect(el.scrollTop).toBe(1000);
  });

  it("keeps following the bottom when content is added", () => {
    const el = scroller(1000, 900);
    const ref = { current: el };
    const { rerender } = renderHook(({ n }) => useAutoScroll(ref, [n], 1), { initialProps: { n: 1 } });
    el.dispatchEvent(new Event("scroll"));
    el.setHeight(1200);
    rerender({ n: 2 });
    expect(el.scrollTop).toBe(1200);
  });

  it("stays put when content is added while the reader is further up", () => {
    const el = scroller(1000, 0);
    const ref = { current: el };
    const { rerender } = renderHook(({ n }) => useAutoScroll(ref, [n], 1), { initialProps: { n: 1 } });
    el.scrollTop = 100;
    el.dispatchEvent(new Event("scroll"));
    el.setHeight(1200);
    rerender({ n: 2 });
    expect(el.scrollTop).toBe(100);
  });

  it("keeps the reader's place when older content is prepended", () => {
    const el = scroller(1000, 0);
    const ref = { current: el };
    const { rerender } = renderHook(({ head }) => useAutoScroll(ref, [head], head), { initialProps: { head: 50 } });
    // The reader scrolled to the top, where older history is loaded.
    el.scrollTop = 0;
    el.dispatchEvent(new Event("scroll"));
    el.setHeight(1700);
    rerender({ head: 30 });
    expect(el.scrollTop).toBe(700);
  });
});
