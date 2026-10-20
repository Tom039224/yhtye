// The settings modal's "リモート接続" section (core-design.md §18): the server
// for the Android client, pairing codes as a QR code, and the paired devices.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { RemoteDevice } from "../api/generated";
import { AppStore } from "../store/app";
import App from "../App";
import { StoreContext } from "../store/useStore";
import { ev } from "../test/events";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { qrPath } from "./QrCode";
import { formatPairingCode, formatRemaining, portError } from "./RemoteSettingsSection";

const PIXEL: RemoteDevice = { id: "dev-0001", name: "Pixel 8", kind: "android", created_ms: Date.UTC(2026, 9, 1, 3, 0), last_seen_ms: Date.UTC(2026, 9, 10, 3, 0), connected: true };
const TABLET: RemoteDevice = { id: "dev-0002", name: "Tablet", kind: "android", created_ms: Date.UTC(2026, 9, 2, 3, 0), last_seen_ms: null, connected: false };

async function setup(configure?: (core: FakeCore) => void, timers = false) {
  const transport = new MemoryTransport();
  const core = new FakeCore(PROJECT, FULL_RUN.start);
  core.attach(transport);
  configure?.(core);
  const store = new AppStore(transport);
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <App initialView="work" />
    </StoreContext.Provider>,
  );
  await act(() => store.openProject(PROJECT.path));
  const user = userEvent.setup(timers ? { advanceTimers: vi.advanceTimersByTime } : {});
  await user.click(await screen.findByRole("button", { name: "設定" }));
  const dialog = await screen.findByRole("dialog", { name: "設定" });
  await user.click(within(dialog).getByRole("button", { name: "リモート接続" }));
  await within(dialog).findByRole("switch", { name: "リモート接続" });
  return { user, dialog, core, transport };
}

/** Starts with the server listening. */
function running(core: FakeCore): void {
  core.remote.set(true, undefined);
}

/** Pushes an app-wide remote event like the core does (no project, live). */
function push(transport: MemoryTransport, body: Parameters<typeof ev>[1]): void {
  act(() => transport.emit(ev(0, body, true, "")));
}

function server(dialog: HTMLElement): HTMLElement {
  return within(dialog).getByRole("region", { name: "サーバー" });
}

function deviceNames(dialog: HTMLElement): string[] {
  const list = within(dialog).queryByRole("list", { name: "端末" });
  return list ? within(list).getAllByRole("listitem").map((li) => li.getAttribute("aria-label") ?? "") : [];
}

afterEach(() => {
  vi.useRealTimers();
});

describe("remote settings helpers", () => {
  it("accepts ports 1-65535 only", () => {
    expect(portError("1423")).toBeNull();
    expect(portError(" 1 ")).toBeNull();
    expect(portError("65535")).toBeNull();
    for (const bad of ["", "0", "65536", "12a", "-1", "1.5", "999999"]) expect(portError(bad), bad).not.toBeNull();
  });

  it("formats the code in halves and the time left", () => {
    expect(formatPairingCode("ABCD2345")).toBe("ABCD-2345");
    expect(formatRemaining(300_000)).toBe("5:00");
    expect(formatRemaining(61_001)).toBe("1:02");
    expect(formatRemaining(-5)).toBe("0:00");
  });
});

describe("remote settings", () => {
  it("shows a stopped server and the devices, with no add button", async () => {
    const { dialog, transport } = await setup((c) => (c.remote.devices = [PIXEL, TABLET]));
    expect(transport.callsOf("get_remote_status")).toHaveLength(1);
    expect(within(dialog).getByRole("switch", { name: "リモート接続" })).not.toBeChecked();
    expect(within(server(dialog)).getByRole("status").textContent).toBe("停止中");
    expect((within(dialog).getByRole("textbox", { name: "ポート" }) as HTMLInputElement).value).toBe("1423");
    expect(within(dialog).queryByRole("button", { name: "端末を追加" })).toBeNull();
    expect(deviceNames(dialog)).toEqual(["Pixel 8", "Tablet"]);
  });

  it("turns the server on, shows where it listens, and turns it off", async () => {
    const { user, dialog, transport } = await setup();
    await user.click(within(dialog).getByRole("switch", { name: "リモート接続" }));
    await waitFor(() => expect(within(server(dialog)).getByRole("status").textContent).toBe("待ち受け中"));
    expect(transport.callsOf("set_remote_settings")).toEqual([{ type: "set_remote_settings", enabled: true, port: 1423 }]);
    expect(within(dialog).getByRole("switch", { name: "リモート接続" })).toBeChecked();
    expect(within(server(dialog)).getByText("100.64.0.7:1423")).toBeTruthy();
    expect(within(server(dialog)).getByText("[fd7a:115c:a1e0::7]:1423")).toBeTruthy();
    expect(within(server(dialog)).getByText("pc.tailnet.ts.net")).toBeTruthy();
    expect(within(dialog).getByRole("button", { name: "端末を追加" })).toBeTruthy();

    await user.click(within(dialog).getByRole("switch", { name: "リモート接続" }));
    await waitFor(() => expect(within(server(dialog)).getByRole("status").textContent).toBe("停止中"));
    expect(transport.callsOf("set_remote_settings")[1]).toEqual({ type: "set_remote_settings", enabled: false });
    expect(within(server(dialog)).queryByText("100.64.0.7:1423")).toBeNull();
  });

  it("turns on with another port, and refuses a bad one without asking the core", async () => {
    const { user, dialog, transport, core } = await setup();
    const port = within(dialog).getByRole("textbox", { name: "ポート" });
    await user.clear(port);
    await user.type(port, "70000");
    await user.click(within(dialog).getByRole("switch", { name: "リモート接続" }));
    expect((await within(server(dialog)).findByRole("alert")).textContent).toContain("1〜65535");
    expect(port).toHaveAttribute("aria-invalid", "true");
    expect(within(dialog).getByRole("switch", { name: "リモート接続" })).not.toBeChecked();
    await user.click(within(dialog).getByRole("button", { name: "ポートを保存" }));
    expect(transport.callsOf("set_remote_settings")).toHaveLength(0);

    await user.clear(port);
    await user.type(port, "1500");
    expect(within(server(dialog)).queryByRole("alert")).toBeNull();
    await user.click(within(dialog).getByRole("switch", { name: "リモート接続" }));
    await waitFor(() => expect(within(server(dialog)).getByText("100.64.0.7:1500")).toBeTruthy());
    expect(transport.callsOf("set_remote_settings")).toEqual([{ type: "set_remote_settings", enabled: true, port: 1500 }]);
    expect(core.remote.port).toBe(1500);
  });

  it("saves a port while stopped", async () => {
    const { user, dialog, transport } = await setup();
    const port = within(dialog).getByRole("textbox", { name: "ポート" });
    await user.clear(port);
    await user.type(port, "2000{Enter}");
    await waitFor(() => expect(transport.callsOf("set_remote_settings")).toEqual([{ type: "set_remote_settings", enabled: false, port: 2000 }]));
    expect(within(dialog).getByRole("switch", { name: "リモート接続" })).not.toBeChecked();
  });

  it("shows why the server did not start, and retries", async () => {
    const { user, dialog, core, transport } = await setup((c) => {
      c.remote.startError = "Tailscale is not running (BackendState Stopped)";
      c.remote.set(true, undefined);
    });
    expect(within(server(dialog)).getByRole("status").textContent).toBe("エラー");
    expect(within(server(dialog)).getByRole("alert").textContent).toContain("Tailscale is not running (BackendState Stopped)");
    expect(within(dialog).getByRole("switch", { name: "リモート接続" })).toBeChecked();
    expect(within(dialog).queryByRole("button", { name: "端末を追加" })).toBeNull();

    core.remote.startError = null;
    await user.click(within(server(dialog)).getByRole("button", { name: "再試行" }));
    await waitFor(() => expect(within(server(dialog)).getByRole("status").textContent).toBe("待ち受け中"));
    expect(transport.callsOf("set_remote_settings")).toEqual([{ type: "set_remote_settings", enabled: true }]);
    expect(within(server(dialog)).queryByRole("alert")).toBeNull();
  });

  it("follows status events (started elsewhere, then stopped)", async () => {
    const { dialog, core, transport } = await setup();
    running(core);
    push(transport, { type: "remote_status_changed", status: core.remote.status() });
    expect(within(server(dialog)).getByRole("status").textContent).toBe("待ち受け中");
    core.remote.set(false, undefined);
    push(transport, { type: "remote_status_changed", status: core.remote.status() });
    expect(within(server(dialog)).getByRole("status").textContent).toBe("停止中");
  });

  it("issues a code: a QR code of its URI, the host, the port and the code to type", async () => {
    const { user, dialog, transport } = await setup(running);
    await user.click(within(dialog).getByRole("button", { name: "端末を追加" }));
    const card = await within(dialog).findByRole("region", { name: "ペアリングコード" });
    expect(transport.callsOf("create_pairing_code")).toHaveLength(1);

    const qr = within(card).getByRole("img", { name: "ペアリング用の QR コード" });
    const uri = "yhtye://pair?v=1&host=pc.tailnet.ts.net&ip=100.64.0.7&port=1423&code=ABCD2345&sid=0123456789abcdef0123456789abcdef";
    expect(qr.querySelector("path")?.getAttribute("d")).toBe(qrPath(uri).path);
    expect(within(card).getByText("pc.tailnet.ts.net")).toBeTruthy();
    expect(within(card).getByText("1423")).toBeTruthy();
    expect(within(card).getByText("ABCD-2345")).toBeTruthy();
    expect(within(card).getByText(/残り [45]:\d\d/)).toBeTruthy();
    expect(within(dialog).queryByRole("button", { name: "端末を追加" })).toBeNull();
  });

  it("offers the IP to type in when MagicDNS is off", async () => {
    const { user, dialog } = await setup((c) => {
      c.remote.dnsName = null;
      running(c);
    });
    expect(within(server(dialog)).getByText("無効 (IP で接続)")).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "端末を追加" }));
    const card = await within(dialog).findByRole("region", { name: "ペアリングコード" });
    expect(within(card).getByText("100.64.0.7")).toBeTruthy();
  });

  it("drops the QR code when the code runs out, and issues a new one", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { user, dialog, core, transport } = await setup(running, true);
    await user.click(within(dialog).getByRole("button", { name: "端末を追加" }));
    const card = await within(dialog).findByRole("region", { name: "ペアリングコード" });
    expect(within(card).getByText(/残り 5:00/)).toBeTruthy();

    act(() => vi.advanceTimersByTime(60_000));
    expect(within(card).getByText(/残り 4:00/)).toBeTruthy();
    act(() => vi.advanceTimersByTime(4 * 60_000 + 1_000));
    expect(within(card).getByText("コードの有効期限が切れました。")).toBeTruthy();
    expect(within(card).queryByRole("img")).toBeNull();
    expect(within(card).queryByText("ABCD-2345")).toBeNull();

    core.remote.nextCode = "WXYZ6789";
    await user.click(within(card).getByRole("button", { name: "新しいコード" }));
    await waitFor(() => expect(within(dialog).getByText("WXYZ-6789")).toBeTruthy());
    expect(transport.callsOf("create_pairing_code")).toHaveLength(2);
    expect(within(dialog).getByRole("img", { name: "ペアリング用の QR コード" })).toBeTruthy();
  });

  it("drops the QR code once a device paired with it, and lists the device", async () => {
    const { user, dialog, core, transport } = await setup(running);
    await user.click(within(dialog).getByRole("button", { name: "端末を追加" }));
    const card = await within(dialog).findByRole("region", { name: "ペアリングコード" });
    // The core reports the new code as the valid one, then (used) none.
    push(transport, { type: "remote_status_changed", status: core.remote.status() });
    expect(within(card).getByRole("img")).toBeTruthy();

    core.remote.pairingExpiresAt = null;
    core.remote.devices = [PIXEL];
    push(transport, { type: "remote_status_changed", status: core.remote.status() });
    push(transport, { type: "remote_devices_changed", devices: [PIXEL] });
    expect(within(card).getByText("このコードは使われたか、無効になりました。")).toBeTruthy();
    expect(within(card).queryByRole("img")).toBeNull();
    expect(within(card).getByRole("button", { name: "新しいコード" })).toBeTruthy();
    expect(deviceNames(dialog)).toEqual(["Pixel 8"]);
  });

  it("keeps a code whose status event arrives after the reply", async () => {
    const { user, dialog, core, transport } = await setup(running);
    // An event from before the code was issued, delivered late: still no code.
    const before = core.remote.status();
    await user.click(within(dialog).getByRole("button", { name: "端末を追加" }));
    const card = await within(dialog).findByRole("region", { name: "ペアリングコード" });
    push(transport, { type: "remote_status_changed", status: before });
    expect(within(card).getByRole("img")).toBeTruthy();
  });

  it("explains a code refused because the server stopped", async () => {
    const { user, dialog, core } = await setup(running);
    core.remote.running = false;
    await user.click(within(dialog).getByRole("button", { name: "端末を追加" }));
    expect((await within(dialog).findByRole("alert")).textContent).toContain("remote access is not running");
    expect(within(dialog).queryByRole("region", { name: "ペアリングコード" })).toBeNull();
  });

  it("shows each device's connection, last connection and pairing date", async () => {
    const { dialog } = await setup((c) => (c.remote.devices = [PIXEL, TABLET]));
    const list = within(dialog).getByRole("list", { name: "端末" });
    const pixel = within(list).getByRole("listitem", { name: "Pixel 8" });
    expect(within(pixel).getByText("接続中")).toBeTruthy();
    expect(within(pixel).getByText(/^最終接続 2026\/10\/10/)).toBeTruthy();
    expect(within(pixel).getByText(/^追加 2026\/10\/01/)).toBeTruthy();
    const tablet = within(list).getByRole("listitem", { name: "Tablet" });
    expect(within(tablet).getByText("未接続")).toBeTruthy();
    expect(within(tablet).getByText("最終接続 なし")).toBeTruthy();
  });

  it("updates the device list from events", async () => {
    const { dialog, transport } = await setup();
    expect(within(dialog).getByText("ペアリングした端末はありません。")).toBeTruthy();
    push(transport, { type: "remote_devices_changed", devices: [TABLET] });
    expect(deviceNames(dialog)).toEqual(["Tablet"]);
    push(transport, { type: "remote_devices_changed", devices: [{ ...TABLET, connected: true, last_seen_ms: Date.UTC(2026, 9, 11, 3, 0) }] });
    const tablet = within(within(dialog).getByRole("list", { name: "端末" })).getByRole("listitem", { name: "Tablet" });
    expect(within(tablet).getByText("接続中")).toBeTruthy();
  });

  it("revokes a device after a confirmation", async () => {
    const { user, dialog, core, transport } = await setup((c) => (c.remote.devices = [PIXEL, TABLET]));
    await user.click(within(dialog).getByRole("button", { name: "Pixel 8 を失効" }));
    expect(within(dialog).getByText(/Pixel 8 を失効しますか/)).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "やめる" }));
    expect(transport.callsOf("revoke_remote_device")).toHaveLength(0);
    expect(deviceNames(dialog)).toEqual(["Pixel 8", "Tablet"]);

    await user.click(within(dialog).getByRole("button", { name: "Pixel 8 を失効" }));
    await user.click(within(dialog).getByRole("button", { name: "失効する" }));
    await waitFor(() => expect(deviceNames(dialog)).toEqual(["Tablet"]));
    expect(transport.callsOf("revoke_remote_device")).toEqual([{ type: "revoke_remote_device", device: "dev-0001" }]);
    expect(core.remote.devices.map((d) => d.id)).toEqual(["dev-0002"]);
  });

  it("shows a failed revocation", async () => {
    const { user, dialog, core } = await setup((c) => (c.remote.devices = [PIXEL]));
    core.remote.devices = [];
    await user.click(within(dialog).getByRole("button", { name: "Pixel 8 を失効" }));
    await user.click(within(dialog).getByRole("button", { name: "失効する" }));
    expect((await within(dialog).findByRole("alert")).textContent).toContain("unknown device dev-0001");
  });
});
