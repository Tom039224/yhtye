import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  HOSTS_EVENT,
  type HostConnection,
  type HostNotice,
  linkStatus,
  PAIR_LINK_EVENT,
  TauriHostsApi,
} from "./remoteHosts";
import { CommandError } from "./transport";

type Handler = (e: { payload: unknown }) => void;

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  unlisten: vi.fn(),
  handlers: new Map<string, Handler>(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

beforeEach(() => {
  tauri.invoke.mockReset();
  tauri.listen.mockReset();
  tauri.unlisten.mockReset();
  tauri.handlers.clear();
  tauri.listen.mockImplementation(async (name: string, handler: Handler) => {
    tauri.handlers.set(name, handler);
    return tauri.unlisten;
  });
});

describe("linkStatus", () => {
  const status = (c: HostConnection) => linkStatus(c);

  it("is open only while connected, and connecting while a connection is being made", () => {
    expect(status({ state: "connected", session: 3 })).toEqual({ state: "open" });
    expect(status({ state: "connecting", attempt: 0 })).toEqual({ state: "connecting" });
  });

  it("is closed with the time to the next try while waiting to reconnect", () => {
    expect(status({ state: "waiting_to_reconnect", attempt: 1, delay_ms: 1000, reason: "refused" })).toEqual({
      state: "closed",
      reason: "refused",
      retryInMs: 1000,
    });
    expect(status({ state: "waiting_to_reconnect", attempt: 1, delay_ms: 500, reason: null })).toMatchObject({
      state: "closed",
      retryInMs: 500,
    });
  });

  it("is closed for good when the host turned the device down or never connected", () => {
    for (const c of [{ state: "revoked" }, { state: "rejected", message: null }, { state: "idle" }] as HostConnection[]) {
      const s = status(c);
      expect(s).toMatchObject({ state: "closed", retryInMs: null });
      expect((s as { reason: string }).reason).not.toBe("");
    }
    expect(status({ state: "rejected", message: "wrong user" })).toMatchObject({ reason: "wrong user" });
  });
});

describe("TauriHostsApi commands", () => {
  const api = new TauriHostsApi();

  it("lists the hosts", async () => {
    tauri.invoke.mockResolvedValue([]);
    expect(await api.list()).toEqual([]);
    expect(tauri.invoke).toHaveBeenCalledWith("yhtye_remote_hosts", undefined);
  });

  it("previews and adds with the input as the shell reads it (camelCase arguments, snake_case inside)", async () => {
    const input = { kind: "manual", host: "100.64.0.7", port: "1423", code: "ABCD2345" } as const;
    tauri.invoke.mockResolvedValue({ hosts: [], port: 1423, server_id: null, default_device_name: "pc" });
    await api.previewPairing(input);
    expect(tauri.invoke).toHaveBeenLastCalledWith("yhtye_remote_preview_pairing", { input });
    await api.add(input, "my laptop");
    expect(tauri.invoke).toHaveBeenLastCalledWith("yhtye_remote_add", { input, deviceName: "my laptop" });
  });

  it("removes and connects by the host's id", async () => {
    tauri.invoke.mockResolvedValue({ token_left_in_keyring: null });
    expect(await api.remove("sid-1")).toEqual({ token_left_in_keyring: null });
    expect(tauri.invoke).toHaveBeenLastCalledWith("yhtye_remote_remove", { serverId: "sid-1" });
    await api.connect("sid-1");
    expect(tauri.invoke).toHaveBeenLastCalledWith("yhtye_remote_connect", { serverId: "sid-1" });
  });

  it("reports the failure of a command as a CommandError", async () => {
    tauri.invoke.mockRejectedValue({ code: "forbidden", message: "the pairing code was refused" });
    const err = await api.add({ kind: "uri", uri: "yhtye://pair?v=1" }, "pc").catch((e: unknown) => e);
    expect(err).toBeInstanceOf(CommandError);
    expect(err).toMatchObject({ code: "forbidden", message: "the pairing code was refused" });
  });

  it("takes the links that came before the listener from the shell", async () => {
    tauri.invoke.mockResolvedValue(["yhtye://pair?v=1"]);
    expect(await api.takePairLinks()).toEqual(["yhtye://pair?v=1"]);
    expect(tauri.invoke).toHaveBeenCalledWith("yhtye_pair_links_ready", undefined);
  });
});

describe("TauriHostsApi events", () => {
  it("hands on the notices and the pairing links, and stops listening on request", async () => {
    const api = new TauriHostsApi();
    const notices: HostNotice[] = [];
    const links: string[] = [];
    const stopNotices = await api.onNotice((n) => notices.push(n));
    const stopLinks = await api.onPairLink((l) => links.push(l));
    tauri.handlers.get(HOSTS_EVENT)?.({ payload: { type: "hosts_changed" } });
    tauri.handlers.get(PAIR_LINK_EVENT)?.({ payload: "yhtye://pair?v=1&ip=100.64.0.7" });
    expect(notices).toEqual([{ type: "hosts_changed" }]);
    expect(links).toEqual(["yhtye://pair?v=1&ip=100.64.0.7"]);
    stopNotices();
    stopLinks();
    expect(tauri.unlisten).toHaveBeenCalledTimes(2);
  });
});
