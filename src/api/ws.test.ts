// @vitest-environment node
// WsTransport against a small in-test WebSocket server speaking the bridge
// protocol (core-design.md §10). The real bridge is Stage 5.

import type { AddressInfo } from "node:net";

import { afterEach, describe, expect, it, vi } from "vitest";
import { type WebSocket as ServerSocket, WebSocketServer } from "ws";

import type { ApiEvent, ApiResponse, WsRequest } from "./generated";
import { bridgeUrl } from "./create";
import type { ConnectionStatus } from "./transport";
import { parseIncoming, WsTransport } from "./ws";

interface Server {
  url: string;
  requests: WsRequest[];
  sockets: ServerSocket[];
  urls: string[];
  close(): Promise<void>;
}

const servers: Server[] = [];
const transports: WsTransport[] = [];

async function startServer(
  reply: (req: WsRequest, socket: ServerSocket) => void,
  port = 0,
): Promise<Server> {
  const wss = new WebSocketServer({ host: "127.0.0.1", port });
  await new Promise<void>((resolve) => wss.once("listening", () => resolve()));
  const server: Server = {
    url: `ws://127.0.0.1:${(wss.address() as AddressInfo).port}/ws`,
    requests: [],
    sockets: [],
    urls: [],
    close: () =>
      new Promise<void>((resolve) => {
        for (const s of server.sockets) s.terminate();
        wss.close(() => resolve());
      }),
  };
  wss.on("connection", (socket, req) => {
    server.sockets.push(socket);
    server.urls.push(req.url ?? "");
    socket.on("message", (data) => {
      const request = JSON.parse(String(data)) as WsRequest;
      server.requests.push(request);
      reply(request, socket);
    });
  });
  servers.push(server);
  return server;
}

function connect(url: string, requestTimeoutMs?: number): { t: WsTransport; statuses: ConnectionStatus[] } {
  const t = new WsTransport(url, { reconnectDelaysMs: [20], requestTimeoutMs });
  transports.push(t);
  const statuses: ConnectionStatus[] = [];
  t.onStatus((s) => statuses.push(s));
  return { t, statuses };
}

async function opened(statuses: ConnectionStatus[]) {
  await vi.waitFor(() => expect(statuses.at(-1)?.state).toBe("open"));
}

const EVENT: ApiEvent = {
  seq: 3,
  ts_ms: 1,
  project: "repo",
  live: false,
  body: { type: "prompted", session: "orchestrator", text: "hi" },
};

afterEach(async () => {
  for (const t of transports.splice(0)) t.close();
  for (const s of servers.splice(0)) await s.close();
});

describe("WsTransport", () => {
  it("matches replies to requests by id, even out of order", async () => {
    const held: [WsRequest, ServerSocket][] = [];
    const server = await startServer((req, socket) => {
      held.push([req, socket]);
      if (held.length < 2) return;
      // Answer the second request first.
      for (const [r, s] of [...held].reverse()) {
        const ok: ApiResponse =
          r.cmd.type === "list_projects" ? { type: "projects", projects: [] } : { type: "accepted" };
        s.send(JSON.stringify({ id: r.id, ok }));
      }
    });
    const { t, statuses } = connect(server.url);
    await opened(statuses);
    const a = t.invoke({ type: "list_projects" });
    const b = t.invoke({ type: "cancel_orchestrator_turn", project: "repo" });
    await expect(a).resolves.toEqual({ type: "projects", projects: [] });
    await expect(b).resolves.toEqual({ type: "accepted" });
    expect(server.requests.map((r) => r.id)).toEqual([1, 2]);
  });

  it("rejects with the core's error code", async () => {
    const server = await startServer((req, socket) =>
      socket.send(JSON.stringify({ id: req.id, err: { code: "not_found", message: "project x is not open" } })),
    );
    const { t, statuses } = connect(server.url);
    await opened(statuses);
    await expect(t.invoke({ type: "get_snapshot", project: "x" })).rejects.toMatchObject({
      code: "not_found",
      message: "project x is not open",
    });
  });

  it("delivers pushed events and ignores malformed messages", async () => {
    const server = await startServer(() => {});
    const { t, statuses } = connect(server.url);
    const got: ApiEvent[] = [];
    t.subscribe((e) => got.push(e));
    await opened(statuses);
    server.sockets[0].send("not json");
    server.sockets[0].send(JSON.stringify({ hello: 1 }));
    server.sockets[0].send(JSON.stringify({ event: EVENT }));
    await vi.waitFor(() => expect(got).toEqual([EVENT]));
  });

  it("fails pending requests on disconnect, reports it, and reconnects", async () => {
    const server = await startServer(() => {});
    const port = Number(new URL(server.url).port);
    const { t, statuses } = connect(server.url);
    await opened(statuses);
    const pending = t.invoke({ type: "list_projects" });
    await vi.waitFor(() => expect(server.requests).toHaveLength(1));
    await server.close();
    servers.splice(servers.indexOf(server), 1);
    await expect(pending).rejects.toMatchObject({ code: "transport" });
    await vi.waitFor(() => expect(statuses.some((s) => s.state === "closed" && s.retryInMs === 20)).toBe(true));
    await expect(t.invoke({ type: "list_projects" })).rejects.toMatchObject({ code: "transport" });

    const again = await startServer((req, socket) =>
      socket.send(JSON.stringify({ id: req.id, ok: { type: "accepted" } })),
    port);
    await vi.waitFor(() => expect(statuses.at(-1)?.state).toBe("open"), { timeout: 3000 });
    await expect(t.invoke({ type: "cancel_orchestrator_turn", project: "p" })).resolves.toEqual({ type: "accepted" });
    expect(again.requests).toHaveLength(1);
  });

  it("times out requests that get no reply", async () => {
    const server = await startServer(() => {});
    const { t, statuses } = connect(server.url, 30);
    await opened(statuses);
    await expect(t.invoke({ type: "list_projects" })).rejects.toThrow(/no reply to list_projects/);
  });

  it("passes the bridge token in the URL and hides it in the target", async () => {
    const server = await startServer(() => {});
    const url = bridgeUrl({ VITE_YHTYE_BRIDGE_URL: server.url, VITE_YHTYE_BRIDGE_TOKEN: "s3cret" });
    const { t, statuses } = connect(url);
    await opened(statuses);
    expect(server.urls[0]).toBe("/ws?token=s3cret");
    expect(t.target).not.toContain("s3cret");
  });
});

describe("parseIncoming", () => {
  it("classifies replies and events", () => {
    expect(parseIncoming(JSON.stringify({ id: 1, ok: { type: "accepted" } }))).toEqual({
      kind: "ok",
      id: 1,
      ok: { type: "accepted" },
    });
    expect(parseIncoming(JSON.stringify({ id: 2, err: { code: "internal", message: "x" } }))?.kind).toBe("err");
    expect(parseIncoming(JSON.stringify({ event: EVENT }))?.kind).toBe("event");
    expect(parseIncoming(JSON.stringify({ id: "1", ok: {} }))).toBeNull();
    expect(parseIncoming(42)).toBeNull();
  });
});
