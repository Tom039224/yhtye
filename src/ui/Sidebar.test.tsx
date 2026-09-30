// The sidebar's section labels and its host footer: the core's host name, a dot
// for the connection, and the time since the core last answered a ping.

import { act, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CommandError } from "../api/transport";
import { AppStore, PING_INTERVAL_MS } from "../store/app";
import { StoreContext } from "../store/useStore";
import { chatSnapshot } from "../test/chats";
import { PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { Sidebar } from "./Sidebar";

let transport: MemoryTransport;
let core: FakeCore;
let store: AppStore;

/** Lets the pings and timers that are due run. */
const advance = (ms: number) => act(() => vi.advanceTimersByTimeAsync(ms));
const footer = () => screen.getByTestId("host");

function mount() {
  transport = new MemoryTransport();
  core = new FakeCore(PROJECT, chatSnapshot());
  core.attach(transport);
  store = new AppStore(transport);
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <Sidebar />
    </StoreContext.Provider>,
  );
}

beforeEach(() => vi.useFakeTimers());

afterEach(() => {
  store.stop();
  vi.useRealTimers();
});

describe("section labels", () => {
  it("names the project pull-down and the branch tree", async () => {
    mount();
    await advance(0);
    const sidebar = screen.getByRole("complementary", { name: "projects" });
    expect(within(sidebar).getByText("Project")).toBeInTheDocument();
    expect(within(sidebar).getByText("Branch")).toBeInTheDocument();
  });
});

describe("the host footer", () => {
  it("shows the host name the core reports and now, with a green dot", async () => {
    mount();
    await advance(0);
    expect(within(footer()).getByText("test-host")).toBeInTheDocument();
    expect(footer()).toHaveTextContent("now");
    const dot = within(footer()).getByRole("img", { name: "接続中" });
    expect(dot).toHaveClass("dot-ok");
    expect(footer()).toHaveAttribute("title", "test-host · 接続中");
  });

  it("waits with a dash and an amber dot until the first answer", async () => {
    transport = new MemoryTransport();
    core = new FakeCore(PROJECT, chatSnapshot());
    core.before = (cmd) => (cmd.type === "ping" ? new Promise<void>(() => {}) : undefined);
    core.attach(transport);
    store = new AppStore(transport);
    store.start();
    render(
      <StoreContext.Provider value={store}>
        <Sidebar />
      </StoreContext.Provider>,
    );
    await advance(0);
    expect(footer()).toHaveTextContent("—");
    expect(within(footer()).getByRole("img", { name: "接続を確認中" })).toHaveClass("dot-busy");
  });

  it("stays now while pings are answered, then counts up seconds and minutes once they stop", async () => {
    mount();
    await advance(PING_INTERVAL_MS * 3);
    expect(footer()).toHaveTextContent("now");
    core.failures.set("ping", new CommandError("internal", "boom"));
    await advance(4_000);
    expect(footer()).toHaveTextContent("now"); // 4 s since the last answer
    await advance(3_000);
    expect(footer()).toHaveTextContent("7s");
    await advance(60_000);
    expect(footer()).toHaveTextContent("1m");
    // The dot turns red when the core stopped answering on an open connection.
    expect(within(footer()).getByRole("img", { name: "応答なし" })).toHaveClass("dot-bad");
  });

  it("goes back to now when the core answers again", async () => {
    mount();
    await advance(0);
    core.failures.set("ping", new CommandError("internal", "boom"));
    await advance(30_000);
    expect(footer()).toHaveTextContent("30s");
    core.failures.delete("ping");
    await advance(PING_INTERVAL_MS);
    expect(footer()).toHaveTextContent("now");
    expect(within(footer()).getByRole("img", { name: "接続中" })).toHaveClass("dot-ok");
  });

  it("shows a lost connection with a red dot and keeps the host name and the time", async () => {
    mount();
    await advance(0);
    act(() => transport.setStatus({ state: "closed", reason: "gone", retryInMs: 500 }));
    await advance(20_000);
    expect(within(footer()).getByRole("img", { name: "切断" })).toHaveClass("dot-bad");
    expect(within(footer()).getByText("test-host")).toBeInTheDocument();
    expect(footer()).toHaveTextContent("20s");
  });
});
