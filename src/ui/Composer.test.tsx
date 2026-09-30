// The composer's icon buttons: send, and stop for a running turn; no inert mode label.
// The text uses the full height and the buttons sit beside it (not under it).

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Composer } from "./Composer";

describe("Composer", () => {
  function setup(props: { turnRunning?: boolean } = {}) {
    const onSend = vi.fn().mockResolvedValue(true);
    const onCancel = vi.fn();
    render(
      <Composer
        disabled={false}
        disabledReason={null}
        sending={false}
        turnRunning={props.turnRunning ?? false}
        mentions={[]}
        onRemoveMention={() => {}}
        onSend={onSend}
        onCancel={onCancel}
      />,
    );
    return { onSend, onCancel, user: userEvent.setup() };
  }

  it("has no mode label, and sends with the send icon once there is text", async () => {
    const { onSend, user } = setup();
    expect(screen.queryByText("orchestrate")).not.toBeInTheDocument();
    const send = screen.getByRole("button", { name: "送信" });
    expect(send).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" }), "do it");
    expect(send).toBeEnabled();
    await user.click(send);
    expect(onSend).toHaveBeenCalledWith("do it");
  });

  it("has no stop icon while no turn runs", () => {
    setup();
    expect(screen.queryByRole("button", { name: "ターンを中止" })).not.toBeInTheDocument();
  });

  it("cancels the running turn with the stop icon", async () => {
    const { onCancel, user } = setup({ turnRunning: true });
    await user.click(screen.getByRole("button", { name: "ターンを中止" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("keeps the textarea and the send button side by side, with no row of their own below", () => {
    setup();
    const textarea = screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" });
    const send = screen.getByRole("button", { name: "送信" });
    const body = textarea.parentElement;
    expect(body).toHaveClass("composer-body");
    expect(body).toContainElement(send);
    // Nothing (no chips, no hint) needs a second row, so the composer is just the one line.
    expect(document.querySelector(".composer-row")).toBeNull();
  });

  it("starts at one row and grows with newlines up to eleven rows", async () => {
    const { user } = setup();
    const textarea = screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" });
    expect(textarea).toHaveAttribute("rows", "1");
    await user.type(textarea, "a{Shift>}{Enter}{/Shift}b{Shift>}{Enter}{/Shift}c");
    expect(textarea).toHaveAttribute("rows", "3");
    await user.clear(textarea);
    await user.type(textarea, "x".concat("{Shift>}{Enter}{/Shift}x".repeat(14)));
    expect(textarea).toHaveAttribute("rows", "11");
  });

  it("puts mention chips on their own row below the text and removes one on click", async () => {
    const onRemove = vi.fn();
    const onSend = vi.fn().mockResolvedValue(true);
    render(
      <Composer
        disabled={false}
        disabledReason={null}
        sending={false}
        turnRunning={false}
        mentions={[{ task: "T-1", title: "fix it" }]}
        onRemoveMention={onRemove}
        onSend={onSend}
        onCancel={() => {}}
      />,
    );
    await userEvent.setup().click(screen.getByRole("button", { name: "T-1 の引用を外す" }));
    expect(onRemove).toHaveBeenCalledWith("T-1");
    expect(document.querySelector(".composer-row")).toContainElement(screen.getByRole("button", { name: "T-1 の引用を外す" }));
  });
});
