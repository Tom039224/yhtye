// Every item of a transcript is a layout root of its own (`.transcript > * { contain: layout style }`), so a
// streamed chunk does not lay out the whole conversation. jsdom computes no layout: this reads the rule, and
// checks the one thing that containment breaks, a `position: fixed` box inside an item.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import type { AttachmentMeta } from "../api/generated";
import appCss from "../App.css?raw";
import { SentAttachments } from "./SentAttachments";

const CSS = appCss.replace(/\/\*[\s\S]*?\*\//g, "");

/** The declarations of the rules whose selector is exactly `selector`. */
function declarationsOf(stripped: string, selector: string): string[] {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\>]/g, "\\$&");
  return [...stripped.matchAll(new RegExp(`(?:^|[},])\\s*${escaped}\\s*\\{([^}]*)\\}`, "g"))].map((m) => m[1].trim());
}

describe("transcript containment", () => {
  it("gives every item layout and style containment, and nothing that clips or guesses sizes", () => {
    const rules = declarationsOf(CSS, ".transcript > *");
    expect(rules).toEqual(["contain: layout style;"]);
    // Paint containment would clip an item's overflow; content-visibility makes the scroll height a guess
    // (WebKit has no scroll anchoring), so the view jumps while scrolling up through items never drawn.
    expect(CSS).not.toMatch(/content-visibility|contain-intrinsic-size/);
    expect(CSS).not.toMatch(/contain:[^;]*\b(paint|strict|content)\b/);
  });

  it("opens an attachment large outside the message (a fixed box in a contained item would sit in the item)", async () => {
    const meta: AttachmentMeta = { id: "a1", name: "shot.png", mime: "image/png", kind: "image", size: 10, delivery: "native" };
    const user = userEvent.setup();
    const { container } = render(
      <div className="transcript">
        <div className="msg msg-user">
          <SentAttachments attachments={[meta]} load={() => Promise.resolve("data:image/png;base64,AAAA")} />
        </div>
      </div>,
    );
    await user.click(await screen.findByRole("button", { name: "shot.png を拡大表示" }));
    const dialog = screen.getByRole("dialog", { name: "shot.png" });
    expect(dialog.parentElement).toBe(document.body);
    expect(container.contains(dialog)).toBe(false);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
