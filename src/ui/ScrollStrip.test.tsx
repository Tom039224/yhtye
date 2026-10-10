import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ScrollStrip } from "./ScrollStrip";

/** jsdom lays nothing out and has no `scrollIntoView`: give the strip a width and spy on the reveal. */
function layout(strip: HTMLElement, { scrollWidth, clientWidth }: { scrollWidth: number; clientWidth: number }) {
  Object.defineProperty(strip, "scrollWidth", { configurable: true, value: scrollWidth });
  Object.defineProperty(strip, "clientWidth", { configurable: true, value: clientWidth });
}

const reveal = vi.fn();

function Strip({ active }: { active: string }) {
  return (
    <ScrollStrip className="row" active={active} data-testid="strip">
      {["a", "b", "c"].map((id) => (
        <button key={id} type="button" className={id === active ? "active" : ""}>
          {id}
        </button>
      ))}
    </ScrollStrip>
  );
}

afterEach(() => {
  reveal.mockReset();
  Reflect.deleteProperty(Element.prototype, "scrollIntoView");
});

describe("ScrollStrip", () => {
  it("scrolls sideways on the vertical wheel and keeps the page from scrolling with it", () => {
    render(<Strip active="a" />);
    const strip = screen.getByTestId("strip");
    layout(strip, { scrollWidth: 500, clientWidth: 100 });
    const notCancelled = fireEvent.wheel(strip, { deltaY: 120 });
    expect(strip.scrollLeft).toBe(120);
    expect(notCancelled).toBe(false);
    fireEvent.wheel(strip, { deltaY: -50 });
    expect(strip.scrollLeft).toBe(70);
  });

  it("counts a wheel in lines or pages by what they move", () => {
    render(<Strip active="a" />);
    const strip = screen.getByTestId("strip");
    layout(strip, { scrollWidth: 500, clientWidth: 100 });
    fireEvent.wheel(strip, { deltaY: 2, deltaMode: 1 });
    expect(strip.scrollLeft).toBe(32);
    fireEvent.wheel(strip, { deltaY: 1, deltaMode: 2 });
    expect(strip.scrollLeft).toBe(132);
  });

  it("leaves the wheel alone when everything fits, and a sideways gesture to the browser", () => {
    render(<Strip active="a" />);
    const strip = screen.getByTestId("strip");
    layout(strip, { scrollWidth: 100, clientWidth: 100 });
    expect(fireEvent.wheel(strip, { deltaY: 120 })).toBe(true);
    expect(strip.scrollLeft).toBe(0);
    layout(strip, { scrollWidth: 500, clientWidth: 100 });
    expect(fireEvent.wheel(strip, { deltaX: 120, deltaY: 10 })).toBe(true);
    expect(strip.scrollLeft).toBe(0);
  });

  it("brings the active item into view when it changes, and only then", () => {
    Object.defineProperty(Element.prototype, "scrollIntoView", { configurable: true, writable: true, value: reveal });
    const { rerender } = render(<Strip active="a" />);
    expect(reveal).toHaveBeenCalledTimes(1);
    expect(reveal.mock.contexts[0]).toBe(screen.getByRole("button", { name: "a" }));
    expect(reveal).toHaveBeenLastCalledWith({ block: "nearest", inline: "nearest" });
    rerender(<Strip active="a" />);
    expect(reveal).toHaveBeenCalledTimes(1);
    rerender(<Strip active="c" />);
    expect(reveal).toHaveBeenCalledTimes(2);
    expect(reveal.mock.contexts[1]).toBe(screen.getByRole("button", { name: "c" }));
  });

  it("copes without an active item or without scrollIntoView", () => {
    expect(() => render(<Strip active="none" />)).not.toThrow();
  });
});
