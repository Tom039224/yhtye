// The status bar's plan name and usage meters (Stage 6b): real values from
// `get_usage`, empty ("—") with the reason when the core cannot report them.
// The connection to the core is not shown here (the sidebar's host footer does).

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../App";
import type { UsageReport } from "../api/generated";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { formatRemaining, planLabel } from "./StatusBar";

const HOUR = 3_600_000;

function report(now: number): UsageReport {
  return {
    plan: "max",
    windows: [
      { kind: "five_hour", label: "5-hour limit", percent: 94, resets_at_ms: now + 3 * HOUR + 35 * 60_000 - 1 },
      { kind: "week", label: "Weekly · all models", percent: 40, resets_at_ms: now + 52 * HOUR - 1 },
      { kind: "other", label: "Weekly · Opus", percent: 3, resets_at_ms: null },
    ],
    fetched_at_ms: now,
  };
}

function setup(usage: UsageReport | null) {
  const transport = new MemoryTransport();
  const core = new FakeCore(PROJECT, FULL_RUN.start);
  core.usage = usage;
  core.attach(transport);
  const store = new AppStore(transport);
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <App />
    </StoreContext.Provider>,
  );
  return { transport, core, store };
}

describe("usage meters", () => {
  it("show the plan, percentages and time to reset from the core", async () => {
    const { transport } = setup(report(Date.now()));
    await waitFor(() => expect(screen.getByTestId("usage-5h")).toHaveTextContent("94%"));
    expect(screen.getByTestId("usage-5h")).toHaveTextContent("5h94%3h 35m");
    expect(screen.getByTestId("usage-week")).toHaveTextContent("7d40%2d 4h");
    expect(screen.getByTestId("plan")).toHaveTextContent("Claude Max");
    // The plan sits in the same group as the meters.
    expect(screen.getByRole("group", { name: "プランと使用量" })).toContainElement(screen.getByTestId("plan"));
    expect(screen.getByRole("group", { name: "プランと使用量" })).toContainElement(screen.getByTestId("usage-5h"));
    const fill = screen.getByTestId("usage-5h").querySelector<HTMLElement>(".meter-fill");
    expect(fill?.style.width).toBe("94%");
    expect(transport.callsOf("get_usage")[0].refresh).toBe(false);
  });

  it("ask the harness again on click", async () => {
    const { transport, core } = setup(report(Date.now()));
    await waitFor(() => expect(screen.getByTestId("usage-week")).toHaveTextContent("40%"));
    core.usage = { ...report(Date.now()), windows: [{ ...report(Date.now()).windows[1], percent: 41 }] };
    await userEvent.setup().click(screen.getByRole("button", { name: /5h.*7d/ }));
    await waitFor(() => expect(screen.getByTestId("usage-week")).toHaveTextContent("41%"));
    expect(transport.callsOf("get_usage").at(-1)?.refresh).toBe(true);
    // A window the harness no longer reports is empty again.
    expect(screen.getByTestId("usage-5h")).toHaveTextContent("5h—");
  });

  it("stay empty with the reason when usage is unavailable", async () => {
    const { store } = setup(null);
    await waitFor(() => expect(store.getState().usage.error).toMatch(/no harness/));
    expect(screen.getByTestId("usage-5h")).toHaveTextContent("5h—");
    expect(screen.getByTestId("usage-week")).toHaveTextContent("7d—");
    expect(screen.queryByTestId("plan")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /5h.*7d/ }).title).toMatch(/使用量を取得できません/);
  });

  it("has no connection item on the left: only the plan and usage group", async () => {
    setup(report(Date.now()));
    await waitFor(() => expect(screen.getByTestId("usage-5h")).toHaveTextContent("94%"));
    const footer = screen.getByRole("group", { name: "プランと使用量" }).closest("footer");
    expect(footer).not.toBeNull();
    expect(screen.queryByTestId("connection")).not.toBeInTheDocument();
    expect(footer?.querySelector(".status-icon")).toBeNull();
    // The only child of the bar besides the spacer is the group on the right.
    expect(Array.from(footer?.children ?? []).filter((el) => !el.classList.contains("spacer"))).toHaveLength(1);
  });

  it("names the plan as the Claude subscription it is", () => {
    expect(planLabel("max")).toBe("Claude Max");
    expect(planLabel("pro")).toBe("Claude Pro");
    expect(planLabel("Team")).toBe("Claude Team");
    expect(planLabel("Claude Enterprise")).toBe("Claude Enterprise");
    expect(planLabel(" max 5x ")).toBe("Claude Max 5x");
  });

  it("formats the time to reset", () => {
    expect(formatRemaining(0)).toBe("リセット済み");
    expect(formatRemaining(-5)).toBe("リセット済み");
    expect(formatRemaining(12 * 60_000)).toBe("12m");
    expect(formatRemaining(HOUR)).toBe("1h 0m");
    expect(formatRemaining(26 * HOUR + 1)).toBe("1d 2h");
  });
});
