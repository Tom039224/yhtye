// The sidebar's section labels and its host footer: the core's host name, a dot
// for the connection, the time since the core last answered a ping, and the
// paired devices under it (T-73).

import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RemoteDevice } from "../api/generated";
import { CommandError } from "../api/transport";
import { AppStore, PING_INTERVAL_MS } from "../store/app";
import { StoreContext } from "../store/useStore";
import { chatSnapshot } from "../test/chats";
import { ev } from "../test/events";
import { PROJECT } from "../test/fixtures";
import { clientOf, FakeCore, MemoryTransport } from "../test/memoryTransport";
import { CLIENTS_SHOWN } from "./hostStatus";
import { Sidebar } from "./Sidebar";

let transport: MemoryTransport;
let core: FakeCore;
let store: AppStore;

/** Lets the pings and timers that are due run. */
const advance = (ms: number) => act(() => vi.advanceTimersByTimeAsync(ms));
const footer = () => screen.getByTestId("host");

function mount(setup?: (core: FakeCore) => void) {
  transport = new MemoryTransport();
  core = new FakeCore(PROJECT, chatSnapshot());
  setup?.(core);
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

  it("marks a host without a GUI with its own icon and says so in the tooltip", async () => {
    mount((c) => {
      c.headless = true;
    });
    await advance(0);
    expect(iconOf(footer())).toBe("server-headless");
    expect(footer()).toHaveAttribute("title", "test-host · ヘッドレス · 接続中");
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

const PIXEL: RemoteDevice = { id: "dev-1", name: "Pixel 9", kind: "android", created_ms: 1, last_seen_ms: Date.UTC(2026, 9, 10, 3, 0), connected: true };
const TABLET: RemoteDevice = { id: "dev-2", name: "Tablet", kind: "android", created_ms: 2, last_seen_ms: Date.UTC(2026, 9, 9, 3, 0), connected: false };
const NEVER: RemoteDevice = { id: "dev-3", name: "Old phone", kind: "android", created_ms: 3, last_seen_ms: null, connected: false };
const LAPTOP: RemoteDevice = { id: "dev-4", name: "Laptop", kind: "desktop", created_ms: 4, last_seen_ms: Date.UTC(2026, 9, 11, 3, 0), connected: true };

const clientList = () => screen.queryByRole("list", { name: "クライアント" });
const clientRows = () => within(clientList() ?? document.createElement("ul")).queryAllByRole("listitem");
const iconOf = (el: HTMLElement) => el.querySelector("svg.client-kind")?.getAttribute("data-icon");

/** Pushes `clients_changed` like the core does (app-wide: no project, live). */
function pushClients(devices: RemoteDevice[]): void {
  const clients = { server: { host: "test-host" }, devices: devices.map(clientOf) };
  act(() => transport.emit(ev(0, { type: "clients_changed", clients }, true, "")));
}

describe("the clients under the host", () => {
  it("marks the server with the server icon and shows no client rows without devices", async () => {
    mount();
    await advance(0);
    expect(iconOf(footer())).toBe("server");
    expect(clientList()).toBeNull();
    expect(transport.calls.some((c) => c.type === "list_clients")).toBe(true);
  });

  it("lists the paired devices with the phone icon, connected first, offline ones greyed out with their last connection", async () => {
    mountWith([NEVER, TABLET, PIXEL]);
    await advance(0);
    const rows = clientRows();
    expect(rows.map((r) => r.textContent)).toEqual(["Pixel 9", "Tablet", "Old phone"]);
    expect(rows.map(iconOf)).toEqual(["phone", "phone", "phone"]);
    expect(rows[0]).not.toHaveClass("offline");
    expect(rows[0]).toHaveAttribute("aria-label", "Pixel 9 · 接続中");
    expect(rows[1]).toHaveClass("offline");
    expect(rows[1].getAttribute("title")).toMatch(/^Tablet · Android · オフライン · 最終接続 2026\/10\/09/);
    expect(rows[2]).toHaveClass("offline");
    expect(rows[2]).toHaveAttribute("title", "Old phone · Android · オフライン · 未接続");
  });

  it("draws another desktop app with the monitor icon and calls it a PC", async () => {
    mountWith([PIXEL, LAPTOP]);
    await advance(0);
    const rows = clientRows();
    // Both are connected, so the one seen last comes first.
    expect(rows.map((r) => r.textContent)).toEqual(["Laptop", "Pixel 9"]);
    expect(rows.map(iconOf)).toEqual(["monitor", "phone"]);
    expect(rows[0].getAttribute("title")).toBe("Laptop · PC · 接続中");
  });

  it("follows clients_changed: a device connects, another is revoked, the last one goes", async () => {
    mountWith([PIXEL, TABLET]);
    await advance(0);
    pushClients([PIXEL, { ...TABLET, connected: true }]);
    expect(clientRows().map((r) => r.className)).toEqual(["client-row", "client-row"]);
    pushClients([{ ...TABLET, connected: true }]);
    expect(clientRows().map((r) => r.textContent)).toEqual(["Tablet"]);
    pushClients([]);
    expect(clientList()).toBeNull();
  });

  it("reads the devices again on a resync and after a reconnect", async () => {
    mountWith([]);
    await advance(0);
    core.remote.devices = [TABLET];
    act(() => transport.emit(ev(0, { type: "resync" }, true, "")));
    await advance(0);
    expect(clientRows().map((r) => r.textContent)).toEqual(["Tablet"]);
    core.remote.devices = [TABLET, PIXEL];
    act(() => transport.setStatus({ state: "closed", reason: "gone", retryInMs: 500 }));
    act(() => transport.setStatus({ state: "open" }));
    await advance(0);
    expect(clientRows().map((r) => r.textContent)).toEqual(["Pixel 9", "Tablet"]);
  });

  it("keeps a clients_changed that came while an older read was on its way", async () => {
    let release = () => {};
    transport = new MemoryTransport();
    core = new FakeCore(PROJECT, chatSnapshot());
    core.remote.devices = [TABLET];
    core.before = (cmd) => (cmd.type === "list_clients" ? new Promise<void>((r) => (release = r)) : undefined);
    mountCore();
    await advance(0);
    pushClients([PIXEL]);
    release();
    await advance(0);
    expect(clientRows().map((r) => r.textContent)).toEqual(["Pixel 9"]);
  });

  it("folds devices past the first three into 他 N 台, and unfolds them in a box of their own", async () => {
    const many = Array.from({ length: 7 }, (_, i): RemoteDevice => ({ ...TABLET, id: `dev-${i}`, name: `Phone ${i}`, last_seen_ms: 1_000 - i }));
    mountWith(many);
    await advance(0);
    expect(clientRows()).toHaveLength(CLIENTS_SHOWN);
    const more = screen.getByRole("button", { name: "他 4 台" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(more);
    expect(clientRows()).toHaveLength(7);
    expect(clientList()).toHaveClass("expanded");
    fireEvent.click(screen.getByRole("button", { name: "折りたたむ" }));
    expect(clientRows()).toHaveLength(CLIENTS_SHOWN);
  });

  it("does not fold exactly three devices", async () => {
    mountWith([PIXEL, TABLET, NEVER]);
    await advance(0);
    expect(clientRows()).toHaveLength(3);
    expect(screen.queryByRole("button", { name: /^他/ })).toBeNull();
  });
});

function mountWith(devices: RemoteDevice[]): void {
  transport = new MemoryTransport();
  core = new FakeCore(PROJECT, chatSnapshot());
  core.remote.devices = devices;
  mountCore();
}

function mountCore(): void {
  core.attach(transport);
  store = new AppStore(transport);
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <Sidebar />
    </StoreContext.Provider>,
  );
}
