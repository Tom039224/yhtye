import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ContextBar, tokens } from "./ContextBar";
import { compactState } from "./Conversation";

describe("ContextBar", () => {
  it("shows a dash until the orchestrator reports its context", () => {
    render(<ContextBar usage={null} compact={{ kind: "ready" }} onCompact={() => {}} />);
    const meter = screen.getByTestId("context-meter");
    expect(meter).toHaveTextContent("—");
    expect(meter).toHaveClass("ctx-meter-empty");
    expect(meter).toHaveAttribute("title", expect.stringContaining("未取得"));
  });

  it("shows the share and the token counts, with the details in the tooltip", () => {
    const usage = { used: 84_000, size: 200_000, cost: { amount: 1.234, currency: "USD" } };
    render(<ContextBar usage={usage} compact={{ kind: "ready" }} onCompact={() => {}} />);
    const meter = screen.getByTestId("context-meter");
    expect(meter).toHaveTextContent("42% · 84k / 200k");
    expect(meter.getAttribute("title")).toContain("84,000 / 200,000 トークン (42%)");
    expect(meter.getAttribute("title")).toContain("累計コスト: 1.23 USD");
    expect(meter).not.toHaveClass("ctx-meter-warn");
  });

  it("warns when the context is nearly full", () => {
    render(<ContextBar usage={{ used: 190_000, size: 200_000, cost: null }} compact={{ kind: "ready" }} onCompact={() => {}} />);
    expect(screen.getByTestId("context-meter")).toHaveClass("ctx-meter-high");
  });

  it("sends the compact command when ready", async () => {
    const onCompact = vi.fn();
    render(<ContextBar usage={null} compact={{ kind: "ready" }} onCompact={onCompact} />);
    const button = screen.getByRole("button", { name: "コンテキストを圧縮" });
    expect(button).toHaveTextContent("圧縮");
    await userEvent.setup().click(button);
    expect(onCompact).toHaveBeenCalledTimes(1);
  });

  it("is disabled with the reason as its tooltip", () => {
    render(<ContextBar usage={null} compact={{ kind: "disabled", reason: "作業中は圧縮できません" }} onCompact={() => {}} />);
    const button = screen.getByTestId("compact-button");
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("title", "作業中は圧縮できません");
  });

  it("reads 圧縮中… and stays disabled while compacting", () => {
    render(<ContextBar usage={null} compact={{ kind: "compacting" }} onCompact={() => {}} />);
    const button = screen.getByRole("button", { name: "圧縮中…" });
    expect(button).toBeDisabled();
    expect(button).toHaveTextContent("圧縮中…");
  });

  it("abbreviates token counts", () => {
    expect([tokens(950), tokens(84_400), tokens(1_000_000), tokens(1_250_000)]).toEqual(["950", "84k", "1M", "1.3M"]);
  });
});

describe("compactState", () => {
  const base = { disabledReason: null, status: "live", turnRunning: false, compacting: false };

  it("is ready only for a live, idle orchestrator", () => {
    expect(compactState(base)).toEqual({ kind: "ready" });
    expect(compactState({ ...base, status: undefined })).toMatchObject({ kind: "disabled", reason: expect.stringContaining("起動していない") });
    expect(compactState({ ...base, status: "stopped" })).toMatchObject({ kind: "disabled" });
    expect(compactState({ ...base, turnRunning: true })).toMatchObject({ kind: "disabled", reason: expect.stringContaining("作業中") });
    expect(compactState({ ...base, disabledReason: "コアに接続していません" })).toEqual({
      kind: "disabled",
      reason: "コアに接続していません",
    });
  });

  it("shows a compaction in progress over everything else", () => {
    expect(compactState({ ...base, turnRunning: true, compacting: true })).toEqual({ kind: "compacting" });
  });
});
