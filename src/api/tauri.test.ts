import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ApiEvent } from "./generated";
import type { HostConnection, RemoteHostInfo } from "./remoteHosts";
import { TAURI_COMMAND, TAURI_EVENT, TauriTransport } from "./tauri";
import { CommandError, type ConnectionStatus } from "./transport";

type Handler = (e: { payload: unknown }) => void;

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  unlisten: vi.fn(),
  handlers: new Map<string, Handler>(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

const EVENT: ApiEvent = { seq: 3, ts_ms: 1, project: "repo", live: false, body: { type: "projects_changed" } as never };

function hostInfo(connection: HostConnection): RemoteHostInfo {
  return {
    server_id: "sid-1",
    name: "far",
    hosts: ["100.64.0.7"],
    port: 1423,
    device_id: "dev",
    device_name: "pc",
    added_ms: 1,
    token_in_keyring: true,
    token: { place: "keyring" },
    status: { connection, server: null },
  };
}

/** Lets the pending `listen` promises settle. */
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

function statuses(t: TauriTransport): ConnectionStatus[] {
  const seen: ConnectionStatus[] = [];
  t.onStatus((s) => seen.push(s));
  return seen;
}

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

describe("TauriTransport commands", () => {
  it("sends a command of the local core without a host", async () => {
    tauri.invoke.mockResolvedValue({ type: "accepted" });
    const t = new TauriTransport();
    expect(await t.invoke({ type: "ping" })).toEqual({ type: "accepted" });
    expect(tauri.invoke).toHaveBeenCalledWith(TAURI_COMMAND, { host: null, cmd: { type: "ping" } });
  });

  it("sends a command of a remote host with the host's id", async () => {
    tauri.invoke.mockResolvedValue({ type: "accepted" });
    const t = new TauriTransport("sid-1");
    await t.invoke({ type: "list_projects" });
    expect(tauri.invoke).toHaveBeenCalledWith(TAURI_COMMAND, { host: "sid-1", cmd: { type: "list_projects" } });
    expect(t.hostId).toBe("sid-1");
  });

  it("rejects with the error the core or the host answered", async () => {
    tauri.invoke.mockRejectedValue({ code: "not_found", message: "no such project" });
    const err = await new TauriTransport("sid-1").invoke({ type: "ping" }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(CommandError);
    expect(err).toMatchObject({ code: "not_found", message: "no such project" });
    tauri.invoke.mockRejectedValue("the shell is gone");
    expect(await new TauriTransport().invoke({ type: "ping" }).catch((e: unknown) => e)).toMatchObject({
      code: "transport",
    });
  });
});

describe("TauriTransport events", () => {
  it("passes on only the events of its own host", async () => {
    const local = new TauriTransport();
    const remote = new TauriTransport("sid-1");
    const other = new TauriTransport("sid-2");
    await settle();
    const got: Record<string, ApiEvent[]> = { local: [], remote: [], other: [] };
    local.subscribe((e) => got.local.push(e));
    remote.subscribe((e) => got.remote.push(e));
    other.subscribe((e) => got.other.push(e));
    // One listener per transport: each one is handed every event.
    const handlers = tauri.listen.mock.calls.map(([, h]) => h as Handler);
    expect(handlers).toHaveLength(3);
    expect(tauri.listen.mock.calls.every(([name]) => name === TAURI_EVENT)).toBe(true);
    const deliver = (host: string | null, event: ApiEvent) => handlers.forEach((h) => h({ payload: { host, event } }));
    deliver(null, EVENT);
    deliver("sid-1", { ...EVENT, seq: 4 });
    deliver("sid-9", { ...EVENT, seq: 5 });
    expect(got.local.map((e) => e.seq)).toEqual([3]);
    expect(got.remote.map((e) => e.seq)).toEqual([4]);
    expect(got.other).toEqual([]);
  });

  it("stops listening when closed", async () => {
    const t = new TauriTransport("sid-1");
    await settle();
    t.close();
    expect(tauri.unlisten).toHaveBeenCalledTimes(1);
    expect(statuses(t)[0]).toMatchObject({ state: "closed" });
  });

  it("stops listening at once if it was closed before the listener was in place", async () => {
    const t = new TauriTransport();
    t.close();
    await settle();
    expect(tauri.unlisten).toHaveBeenCalledTimes(1);
  });
});

describe("TauriTransport status of the local core", () => {
  it("is connecting until the events can be heard, then open", async () => {
    const t = new TauriTransport();
    const seen = statuses(t);
    expect(seen).toEqual([{ state: "connecting" }]);
    await settle();
    expect(seen).toEqual([{ state: "connecting" }, { state: "open" }]);
  });

  it("is closed, with why, when the events cannot be heard", async () => {
    tauri.listen.mockRejectedValue(new Error("no ipc"));
    const t = new TauriTransport();
    const seen = statuses(t);
    await settle();
    expect(seen.at(-1)).toMatchObject({ state: "closed", retryInMs: null });
    expect((seen.at(-1) as { reason: string }).reason).toContain("no ipc");
  });

  it("ignores what the shell says of remote hosts", async () => {
    const t = new TauriTransport();
    await settle();
    t.applyHost(hostInfo({ state: "revoked" }));
    expect(statuses(t)[0]).toEqual({ state: "open" });
  });
});

describe("TauriTransport status of a remote host", () => {
  it("follows the shell's connection to the host once the events can be heard", async () => {
    const t = new TauriTransport("sid-1");
    const seen = statuses(t);
    t.applyHost(hostInfo({ state: "connected", session: 1 }));
    // Connected, but nothing listens yet: not open.
    expect(seen.at(-1)).toEqual({ state: "connecting" });
    await settle();
    expect(seen.at(-1)).toEqual({ state: "open" });

    t.applyHost(hostInfo({ state: "waiting_to_reconnect", attempt: 2, delay_ms: 2000, reason: "refused" }));
    expect(seen.at(-1)).toEqual({ state: "closed", reason: "refused", retryInMs: 2000 });
    t.applyHost(hostInfo({ state: "connecting", attempt: 2 }));
    expect(seen.at(-1)).toEqual({ state: "connecting" });
    t.applyHost(hostInfo({ state: "connected", session: 2 }));
    expect(seen.at(-1)).toEqual({ state: "open" });
  });

  it("does not try again by itself once the host turned the device down", async () => {
    const t = new TauriTransport("sid-1");
    await settle();
    const seen = statuses(t);
    t.applyHost(hostInfo({ state: "revoked" }));
    expect(seen.at(-1)).toMatchObject({ state: "closed", retryInMs: null });
    t.applyHost(hostInfo({ state: "rejected", message: "other user" }));
    expect(seen.at(-1)).toEqual({ state: "closed", reason: "other user", retryInMs: null });
    t.applyHost(hostInfo({ state: "idle" }));
    expect(seen.at(-1)).toMatchObject({ state: "closed", retryInMs: null });
  });

  it("shows a new connection that came with no break in between as a break, so that the store reads its state again", async () => {
    const t = new TauriTransport("sid-1");
    await settle();
    t.applyHost(hostInfo({ state: "connected", session: 1 }));
    const seen = statuses(t);
    seen.length = 0;
    t.applyHost(hostInfo({ state: "connected", session: 1 }));
    expect(seen).toEqual([]);
    t.applyHost(hostInfo({ state: "connected", session: 2 }));
    expect(seen).toEqual([{ state: "connecting" }, { state: "open" }]);
  });
});
