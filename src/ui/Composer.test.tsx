// The composer's icon buttons: send, and stop for a running turn; no inert mode label.
// The text uses the full height and the buttons sit beside it (not under it).

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

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
    const body = textarea.closest(".composer-body");
    expect(body).toHaveClass("composer-body");
    expect(body).toContainElement(send);
    // Nothing (no chips, no hint) needs a second row, so the composer is just the one line.
    expect(document.querySelector(".composer-row")).toBeNull();
  });

  describe("height", () => {
    const BOX = { name: "オーケストレータへのメッセージ" };
    const ROW = 21;
    const lines = (el: HTMLElement) => (el.textContent ?? "").split("\n").length;
    const scrollHeight = vi.spyOn(Element.prototype, "scrollHeight", "get");

    // jsdom lays nothing out: the mirror is as tall as its lines, and the textarea must never be measured.
    function mockLayout() {
      scrollHeight.mockImplementation(function (this: HTMLElement) {
        if (this instanceof HTMLTextAreaElement) throw new Error("the textarea itself must not be measured");
        return this.classList.contains("composer-field-mirror") ? lines(this) * ROW : 0;
      });
    }
    afterEach(() => scrollHeight.mockReset());

    it("starts at one row and follows the text, newlines or wrapped lines alike", async () => {
      mockLayout();
      const { user } = setup();
      const textarea = screen.getByRole("textbox", BOX);
      expect(textarea.style.height).toBe(`${ROW}px`);
      await user.type(textarea, "a{Shift>}{Enter}{/Shift}b{Shift>}{Enter}{/Shift}c");
      expect(textarea.style.height).toBe(`${3 * ROW}px`);
      await user.type(textarea, "{Backspace}{Backspace}");
      expect(textarea.style.height).toBe(`${2 * ROW}px`);
    });

    it("gives a trailing newline its row (the mirror has a space on it)", async () => {
      mockLayout();
      const { user } = setup();
      await user.type(screen.getByRole("textbox", BOX), "a{Shift>}{Enter}{/Shift}");
      expect(document.querySelector(".composer-field-mirror")?.textContent).toBe("a\n ");
    });

    it("goes back to one row once the message is sent", async () => {
      mockLayout();
      const { user } = setup();
      const textarea = screen.getByRole("textbox", BOX);
      await user.type(textarea, "a{Shift>}{Enter}{/Shift}b{Shift>}{Enter}{/Shift}c");
      expect(textarea.style.height).toBe(`${3 * ROW}px`);
      await user.click(screen.getByRole("button", { name: "送信" }));
      expect(textarea).toHaveValue("");
      expect(textarea.style.height).toBe(`${ROW}px`);
    });

    it("keeps the mirror out of the accessibility tree", () => {
      setup();
      const mirror = document.querySelector(".composer-field-mirror");
      expect(mirror).toHaveAttribute("aria-hidden", "true");
      expect(screen.queryAllByRole("textbox", BOX)).toHaveLength(1);
    });

    it("leaves the height alone while nothing is laid out", async () => {
      const { user } = setup();
      const textarea = screen.getByRole("textbox", BOX);
      await user.type(textarea, "a{Shift>}{Enter}{/Shift}b");
      expect(textarea.style.height).toBe("");
    });
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

  describe("draft", () => {
    const BOX = { name: "オーケストレータへのメッセージ" };

    function draftSetup(initialText: string, sent = true) {
      const onSend = vi.fn().mockResolvedValue(sent);
      const onTextChange = vi.fn();
      render(
        <Composer
          disabled={false}
          disabledReason={null}
          sending={false}
          turnRunning={false}
          initialText={initialText}
          onTextChange={onTextChange}
          mentions={[]}
          onRemoveMention={() => {}}
          onSend={onSend}
          onCancel={() => {}}
        />,
      );
      return { onSend, onTextChange, user: userEvent.setup() };
    }

    it("starts with the saved draft", () => {
      draftSetup("half-written");
      expect(screen.getByRole("textbox", BOX)).toHaveValue("half-written");
    });

    it("reports every edit so the draft can be saved", async () => {
      const { onTextChange, user } = draftSetup("");
      await user.type(screen.getByRole("textbox", BOX), "abc");
      expect(onTextChange.mock.calls.map((c) => c[0])).toEqual(["a", "ab", "abc"]);
    });

    it("drops the draft once the send is accepted", async () => {
      const { onSend, onTextChange, user } = draftSetup("saved text");
      await user.click(screen.getByRole("button", { name: "送信" }));
      expect(onSend).toHaveBeenCalledWith("saved text");
      expect(screen.getByRole("textbox", BOX)).toHaveValue("");
      expect(onTextChange).toHaveBeenLastCalledWith("");
    });

    it("keeps the draft when the send is not accepted", async () => {
      const { onSend, onTextChange, user } = draftSetup("saved text", false);
      await user.click(screen.getByRole("button", { name: "送信" }));
      expect(onSend).toHaveBeenCalledTimes(1);
      expect(screen.getByRole("textbox", BOX)).toHaveValue("saved text");
      expect(onTextChange).not.toHaveBeenCalled();
    });
  });
});
