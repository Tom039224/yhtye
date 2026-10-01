import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Orientation, Side } from "./layout";
import { Resizer } from "./Resizer";

/** A handle over a panel that is `start` px, with limits 100..500; `onResize` moves the panel like the parent does. */
function setup(o: { orientation?: Orientation; side?: Side; start?: number } = {}) {
  let size = o.start ?? 300;
  const onResize = vi.fn((px: number) => {
    size = px;
  });
  const onCommit = vi.fn();
  const view = render(
    <Resizer
      orientation={o.orientation ?? "vertical"}
      side={o.side ?? "before"}
      label="幅"
      measure={() => size}
      bounds={() => ({ min: 100, max: 500 })}
      onResize={onResize}
      onCommit={onCommit}
    />,
  );
  return { handle: screen.getByRole("separator", { name: "幅" }), onResize, onCommit, view };
}

const down = (el: Element, at: { clientX?: number; clientY?: number; button?: number } = {}) =>
  fireEvent.pointerDown(el, { pointerId: 1, button: 0, clientX: 0, clientY: 0, ...at });
const move = (el: Element, at: { clientX?: number; clientY?: number }) => fireEvent.pointerMove(el, { pointerId: 1, ...at });
const up = (el: Element) => fireEvent.pointerUp(el, { pointerId: 1 });

describe("Resizer", () => {
  it("is a focusable separator that reports the panel's size and limits", () => {
    const { handle } = setup();
    expect(handle).toHaveAttribute("aria-orientation", "vertical");
    expect(handle).toHaveAttribute("aria-valuenow", "300");
    expect(handle).toHaveAttribute("aria-valuemin", "100");
    expect(handle).toHaveAttribute("aria-valuemax", "500");
    expect(handle).toHaveAttribute("tabindex", "0");
  });

  it("leaves the value out while the panel is not laid out", () => {
    render(<Resizer orientation="horizontal" side="after" label="高さ" measure={() => 0} bounds={() => ({ min: 1, max: 2 })} onResize={vi.fn()} onCommit={vi.fn()} />);
    const handle = screen.getByRole("separator", { name: "高さ" });
    expect(handle).toHaveAttribute("aria-orientation", "horizontal");
    expect(handle).not.toHaveAttribute("aria-valuenow");
  });

  describe("pointer", () => {
    it("resizes as the pointer moves and commits once, where it ends", () => {
      const { handle, onResize, onCommit } = setup();
      down(handle, { clientX: 100 });
      move(handle, { clientX: 130 });
      move(handle, { clientX: 160 });
      expect(onResize.mock.calls).toEqual([[330], [360]]);
      // Nothing is saved while the handle is held.
      expect(onCommit).not.toHaveBeenCalled();
      expect(handle).toHaveClass("dragging");
      expect(handle).toHaveAttribute("aria-valuenow", "360");
      up(handle);
      expect(onCommit.mock.calls).toEqual([[360]]);
      expect(handle).not.toHaveClass("dragging");
    });

    it("stops at the limits however far the pointer goes", () => {
      const { handle, onResize, onCommit } = setup();
      down(handle, { clientX: 0 });
      move(handle, { clientX: 9999 });
      move(handle, { clientX: -9999 });
      up(handle);
      expect(onResize.mock.calls).toEqual([[500], [100]]);
      expect(onCommit).toHaveBeenCalledWith(100);
    });

    it("grows a panel after the handle as the pointer moves back", () => {
      const { handle, onCommit } = setup({ side: "after" });
      down(handle, { clientX: 100 });
      move(handle, { clientX: 60 });
      up(handle);
      expect(onCommit).toHaveBeenCalledWith(340);
    });

    it("follows the vertical axis for a horizontal handle", () => {
      const { handle, onCommit } = setup({ orientation: "horizontal", side: "after" });
      down(handle, { clientX: 10, clientY: 400 });
      move(handle, { clientX: 500, clientY: 350 });
      up(handle);
      expect(onCommit).toHaveBeenCalledWith(350);
    });

    it("commits nothing for a click that did not move", () => {
      const { handle, onResize, onCommit } = setup();
      down(handle, { clientX: 100 });
      up(handle);
      expect(onResize).not.toHaveBeenCalled();
      expect(onCommit).not.toHaveBeenCalled();
    });

    it("still commits a drag that came back to where it began", () => {
      const { handle, onCommit } = setup();
      down(handle, { clientX: 100 });
      move(handle, { clientX: 150 });
      move(handle, { clientX: 100 });
      up(handle);
      expect(onCommit).toHaveBeenCalledWith(300);
    });

    it("ends the gesture when the pointer is cancelled", () => {
      const { handle, onCommit } = setup();
      down(handle, { clientX: 100 });
      move(handle, { clientX: 120 });
      fireEvent.pointerCancel(handle, { pointerId: 1 });
      expect(onCommit).toHaveBeenCalledWith(320);
      move(handle, { clientX: 200 });
      expect(onCommit).toHaveBeenCalledTimes(1);
    });

    it("ignores other buttons, other pointers and moves without a press", () => {
      const { handle, onResize, onCommit } = setup();
      move(handle, { clientX: 200 });
      down(handle, { button: 2, clientX: 100 });
      move(handle, { clientX: 200 });
      down(handle, { clientX: 100 });
      fireEvent.pointerMove(handle, { pointerId: 2, clientX: 200 });
      fireEvent.pointerUp(handle, { pointerId: 2 });
      expect(onResize).not.toHaveBeenCalled();
      expect(onCommit).not.toHaveBeenCalled();
    });

    it("keeps the resize cursor on the whole page while it is held", () => {
      const { handle, view } = setup();
      down(handle, { clientX: 100 });
      expect(document.body).toHaveClass("resizing-col");
      up(handle);
      expect(document.body).not.toHaveClass("resizing-col");
      down(handle, { clientX: 100 });
      view.unmount();
      expect(document.body).not.toHaveClass("resizing-col");
    });

    it("uses the row cursor for a horizontal handle", () => {
      const { handle } = setup({ orientation: "horizontal" });
      down(handle);
      expect(document.body).toHaveClass("resizing-row");
      up(handle);
      expect(document.body).not.toHaveClass("resizing-row");
    });
  });

  describe("keyboard", () => {
    it("moves the boundary with the arrows and commits each step", () => {
      const { handle, onResize, onCommit } = setup();
      fireEvent.keyDown(handle, { key: "ArrowRight" });
      expect(onResize).toHaveBeenLastCalledWith(316);
      expect(onCommit).toHaveBeenLastCalledWith(316);
      fireEvent.keyDown(handle, { key: "ArrowLeft", shiftKey: true });
      expect(onCommit).toHaveBeenLastCalledWith(252);
      expect(handle).toHaveAttribute("aria-valuenow", "252");
    });

    it("uses the up and down arrows for a horizontal handle", () => {
      const { handle, onCommit } = setup({ orientation: "horizontal", side: "after" });
      fireEvent.keyDown(handle, { key: "ArrowUp" });
      expect(onCommit).toHaveBeenLastCalledWith(316);
      fireEvent.keyDown(handle, { key: "ArrowDown" });
      expect(onCommit).toHaveBeenLastCalledWith(300);
    });

    it("goes to the limits with Home and End and stops there", () => {
      const { handle, onCommit } = setup();
      fireEvent.keyDown(handle, { key: "End" });
      expect(onCommit).toHaveBeenLastCalledWith(500);
      fireEvent.keyDown(handle, { key: "ArrowRight" });
      expect(onCommit).toHaveBeenCalledTimes(1);
      fireEvent.keyDown(handle, { key: "Home" });
      expect(onCommit).toHaveBeenLastCalledWith(100);
    });

    it("ignores keys it does not use", () => {
      const { handle, onResize, onCommit } = setup();
      fireEvent.keyDown(handle, { key: "ArrowUp" });
      fireEvent.keyDown(handle, { key: "a" });
      expect(onResize).not.toHaveBeenCalled();
      expect(onCommit).not.toHaveBeenCalled();
    });
  });

  describe("reset", () => {
    it("goes back to the default on a double click", () => {
      const { handle, onCommit } = setup();
      fireEvent.doubleClick(handle);
      expect(onCommit.mock.calls).toEqual([[null]]);
    });

    it("goes back to the default on Enter", () => {
      const { handle, onCommit } = setup();
      fireEvent.keyDown(handle, { key: "Enter" });
      expect(onCommit.mock.calls).toEqual([[null]]);
    });
  });
});
