// The icon set: decorative SVGs, and buttons that always carry a name and a tooltip.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Icon, ICON_NAMES, IconButton } from "./Icon";

describe("Icon", () => {
  it("is hidden from assistive tech and sized in px", () => {
    const { container } = render(<Icon name="send" size={20} />);
    const svg = container.querySelector("svg");
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).toHaveAttribute("width", "20");
    expect(svg).toHaveAttribute("stroke", "currentColor");
  });
});

describe("the icon set", () => {
  it("draws every glyph on the shared 24px grid with the same stroke", () => {
    expect(ICON_NAMES.length).toBeGreaterThan(40);
    for (const name of ICON_NAMES) {
      const { container, unmount } = render(<Icon name={name} />);
      const svg = container.querySelector("svg");
      expect(svg, name).toHaveAttribute("viewBox", "0 0 24 24");
      expect(svg, name).toHaveAttribute("stroke-width", "1.75");
      expect(svg?.querySelector("path, circle, rect"), name).not.toBeNull();
      unmount();
    }
  });

  it("turns only when asked to", () => {
    const { container } = render(<Icon name="loader" spin />);
    expect(container.querySelector("svg")).toHaveClass("icon-spin");
  });
});

describe("IconButton", () => {
  it("names the button and uses the label as its tooltip", async () => {
    const onClick = vi.fn();
    render(<IconButton icon="refresh" label="読み直す" onClick={onClick} />);
    const button = screen.getByRole("button", { name: "読み直す" });
    expect(button).toHaveAttribute("title", "読み直す");
    expect(button).toHaveAttribute("type", "button");
    await userEvent.setup().click(button);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("keeps the name but shows a longer tooltip, and does not fire when disabled", async () => {
    const onClick = vi.fn();
    render(<IconButton icon="send" tone="primary" label="送信" title="送信 (Enter)" disabled onClick={onClick} />);
    const button = screen.getByRole("button", { name: "送信" });
    expect(button).toHaveAttribute("title", "送信 (Enter)");
    expect(button).toHaveClass("icon-button-primary");
    await userEvent.setup().click(button);
    expect(onClick).not.toHaveBeenCalled();
  });
});
