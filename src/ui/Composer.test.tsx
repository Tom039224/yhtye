// The composer's icon buttons: send, and stop for a running turn; no inert mode label.

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
});
