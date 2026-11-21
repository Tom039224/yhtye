// The composer's height fit (composerField.ts). jsdom lays nothing out, so the measurement is injected;
// the page that proves the cost is `pnpm bench:long-chat` (the INPUT table, `ms/char`).

import { act, render } from "@testing-library/react";
import { useRef } from "react";
import { describe, expect, it, vi } from "vitest";

import appCss from "../App.css?raw";
import { type FieldMetrics, mirrorText, nextFieldHeight, useFieldHeight } from "./composerField";

describe("mirrorText", () => {
  it("gives a line break at the end a line of its own, as the textarea's caret has", () => {
    expect(mirrorText("")).toBe(" ");
    expect(mirrorText("abc")).toBe("abc ");
    expect(mirrorText("a\n")).toBe("a\n ");
  });
});

describe("nextFieldHeight", () => {
  const metrics = (content: number, max = Infinity): FieldMetrics => ({ content, max });

  it("takes the content's height", () => {
    expect(nextFieldHeight(metrics(63), null)).toBe(63);
    expect(nextFieldHeight(metrics(21), 63)).toBe(21);
  });

  it("stops at the maximum, so growing past it changes nothing", () => {
    expect(nextFieldHeight(metrics(300, 228.8), 100)).toBe(228.8);
    expect(nextFieldHeight(metrics(400, 228.8), 228.8)).toBeNull();
  });

  it("leaves the height alone when it is already right", () => {
    expect(nextFieldHeight(metrics(21), 21)).toBeNull();
  });

  it("leaves the height alone when nothing is laid out (a hidden composer measures 0)", () => {
    expect(nextFieldHeight(metrics(0), 21)).toBeNull();
    expect(nextFieldHeight(metrics(Number.NaN), null)).toBeNull();
  });
});

describe("useFieldHeight", () => {
  function Field({ text, measure }: { text: string; measure: (a: HTMLTextAreaElement, b: HTMLElement) => FieldMetrics }) {
    const textarea = useRef<HTMLTextAreaElement>(null);
    const mirror = useRef<HTMLDivElement>(null);
    useFieldHeight(textarea, mirror, text, measure);
    return (
      <div data-testid="slot">
        <textarea ref={textarea} aria-label="field" value={text} readOnly />
        <div ref={mirror}>{mirrorText(text)}</div>
      </div>
    );
  }

  /** `height` as the textarea's inline style gets it: every write, in order. */
  function recordHeightWrites() {
    const probe = document.createElement("textarea");
    const proto = Object.getPrototypeOf(probe.style) as object;
    const original = Object.getOwnPropertyDescriptor(proto, "height");
    if (!original?.set || !original.get) throw new Error("jsdom has no `height` accessor on its style declaration");
    const writes: string[] = [];
    Object.defineProperty(proto, "height", {
      configurable: true,
      get: original.get,
      set(value: string) {
        writes.push(value);
        original.set?.call(this, value);
      },
    });
    return { writes, restore: () => Object.defineProperty(proto, "height", original) };
  }

  it("writes the height only when it changes, not once per key", () => {
    const rec = recordHeightWrites();
    try {
      const measure = (_: HTMLTextAreaElement, mirror: HTMLElement) => ({ content: (mirror.textContent ?? "").split("\n").length * 21, max: 63 });
      const { rerender, getByLabelText } = render(<Field text="" measure={measure} />);
      for (const text of ["a", "ab", "abc", "abcd"]) rerender(<Field text={text} measure={measure} />);
      expect(rec.writes).toEqual(["21px"]);
      rerender(<Field text={"abcd\ne"} measure={measure} />);
      rerender(<Field text={"abcd\ne\nf\ng\nh"} measure={measure} />);
      rerender(<Field text={"abcd\ne\nf\ng\nh\ni"} measure={measure} />);
      expect(rec.writes).toEqual(["21px", "42px", "63px"]);
      expect(getByLabelText("field")).toHaveStyle({ height: "63px" });
    } finally {
      rec.restore();
    }
  });

  it("measures the mirror, never the textarea", () => {
    const measure = vi.fn((_: HTMLTextAreaElement, mirror: HTMLElement) => ({ content: mirror.textContent ? 21 : 0, max: Infinity }));
    render(<Field text="abc" measure={measure} />);
    expect(measure).toHaveBeenCalled();
    for (const [, mirror] of measure.mock.calls) expect(mirror.tagName).toBe("DIV");
  });

  it("fits again when the field's width changes (the text wraps differently)", () => {
    let resized: () => void = () => undefined;
    const observe = vi.fn();
    const disconnect = vi.fn();
    class FakeResizeObserver {
      constructor(callback: () => void) {
        resized = callback;
      }
      observe = observe;
      disconnect = disconnect;
      unobserve = vi.fn();
    }
    vi.stubGlobal("ResizeObserver", FakeResizeObserver);
    try {
      let content = 21;
      const measure = () => ({ content, max: Infinity });
      const { getByLabelText, getByTestId, unmount } = render(<Field text="a long line" measure={measure} />);
      expect(observe).toHaveBeenCalledWith(getByTestId("slot"));
      expect(getByLabelText("field")).toHaveStyle({ height: "21px" });
      content = 42;
      act(() => resized());
      expect(getByLabelText("field")).toHaveStyle({ height: "42px" });
      unmount();
      expect(disconnect).toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("the field's stylesheet", () => {
  const rule = (selector: string) => {
    const match = new RegExp(`(?:^|\\})\\s*${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`, "m").exec(appCss);
    if (!match) throw new Error(`no rule for ${selector}`);
    return match[1].replace(/\s+/g, " ");
  };

  it("keeps the mirror out of flow and as tall as nothing, so measuring it lays out nothing else", () => {
    const mirror = rule(".composer-field-mirror");
    expect(mirror).toContain("position: absolute");
    expect(mirror).toContain("height: 0");
    expect(mirror).toContain("overflow: hidden");
    expect(mirror).toContain("visibility: hidden");
  });

  it("gives the textarea a width and no auto height, with the row limit as its max-height", () => {
    expect(rule(".composer-field > textarea, .composer-field-mirror")).toContain("width: 100%");
    expect(rule(".composer textarea")).toContain("max-height: calc(var(--composer-max-rows) * 1.6em)");
  });
});
