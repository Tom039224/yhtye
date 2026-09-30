import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { useAutoScroll } from "./useAutoScroll";

/** An element with a fixed scroll height (jsdom has no layout). */
function scroller(height: number, top: number) {
  const el = document.createElement("div");
  Object.defineProperty(el, "scrollHeight", { value: height, configurable: true });
  Object.defineProperty(el, "clientHeight", { value: 100, configurable: true });
  el.scrollTop = top;
  return el;
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
});
