// WebSocket transport to the development bridge (core-design.md §10):
// requests `{id, cmd}` → replies `{id, ok}` / `{id, err}`; events `{event}`.
// Reconnects with backoff; every state change is reported through onStatus so
// the UI can show it (nothing fails silently).

import type { ApiCommand, ApiEvent, ApiResponse, WsRequest } from "./generated";
import {
  CommandError,
  type ConnectionStatus,
  Listeners,
  type Transport,
  toCommandError,
} from "./transport";

/** The subset of the browser WebSocket used here (injectable for tests). */
export interface WebSocketLike {
  readonly readyState: number;
  send(data: string): void;
  close(): void;
  onopen: (() => void) | null;
  onclose: ((ev: { code: number; reason: string }) => void) | null;
  onerror: (() => void) | null;
  onmessage: ((ev: { data: unknown }) => void) | null;
}

export type WebSocketFactory = (url: string) => WebSocketLike;

export interface WsOptions {
  createSocket?: WebSocketFactory;
  /** Delays between reconnection attempts; the last one repeats. */
  reconnectDelaysMs?: number[];
  /** A request without a reply after this long fails. */
  requestTimeoutMs?: number;
}

const OPEN = 1;
const DEFAULT_DELAYS_MS = [500, 1000, 2000, 5000];
// Opening a project starts an agent (npx may download it first).
const DEFAULT_REQUEST_TIMEOUT_MS = 180_000;

interface Pending {
  resolve: (r: ApiResponse) => void;
  reject: (e: CommandError) => void;
  timer: ReturnType<typeof setTimeout>;
}

type Incoming =
  | { kind: "event"; event: ApiEvent }
  | { kind: "ok"; id: number; ok: ApiResponse }
  | { kind: "err"; id: number; err: unknown };

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

/** Classifies a bridge message; `null` for anything malformed. */
export function parseIncoming(data: unknown): Incoming | null {
  if (typeof data !== "string") return null;
  let msg: unknown;
  try {
    msg = JSON.parse(data);
  } catch {
    return null;
  }
  if (!isObject(msg)) return null;
  if (isObject(msg.event) && typeof msg.event.seq === "number") {
    return { kind: "event", event: msg.event as ApiEvent };
  }
  if (typeof msg.id !== "number") return null;
  if (isObject(msg.ok) && typeof msg.ok.type === "string") {
    return { kind: "ok", id: msg.id, ok: msg.ok as ApiResponse };
  }
  if ("err" in msg) return { kind: "err", id: msg.id, err: msg.err };
  return null;
}

/** Adapts the platform WebSocket to {@link WebSocketLike}. */
export function browserSocket(url: string): WebSocketLike {
  const ws = new WebSocket(url);
  const like: WebSocketLike = {
    get readyState() {
      return ws.readyState;
    },
    send: (data) => ws.send(data),
    close: () => ws.close(),
    onopen: null,
    onclose: null,
    onerror: null,
    onmessage: null,
  };
  ws.onopen = () => like.onopen?.();
  ws.onclose = (e) => like.onclose?.({ code: e.code, reason: e.reason });
  ws.onerror = () => like.onerror?.();
  ws.onmessage = (e) => like.onmessage?.({ data: e.data });
  return like;
}

export class WsTransport implements Transport {
  readonly kind = "websocket";
  readonly target: string;
  private readonly url: string;
  private readonly createSocket: WebSocketFactory;
  private readonly delays: number[];
  private readonly requestTimeoutMs: number;
  private readonly events = new Listeners<ApiEvent>();
  private readonly statuses = new Listeners<ConnectionStatus>();
  private readonly pending = new Map<number, Pending>();
  private socket: WebSocketLike | null = null;
  private status: ConnectionStatus = { state: "connecting" };
  private nextId = 1;
  private attempt = 0;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private closed = false;

  constructor(url: string, options: WsOptions = {}) {
    this.url = url;
    this.target = url.replace(/token=[^&]*/, "token=…");
    this.createSocket = options.createSocket ?? browserSocket;
    this.delays = options.reconnectDelaysMs ?? DEFAULT_DELAYS_MS;
    this.requestTimeoutMs = options.requestTimeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
    this.connect();
  }

  invoke(cmd: ApiCommand): Promise<ApiResponse> {
    const socket = this.socket;
    if (!socket || socket.readyState !== OPEN || this.status.state !== "open") {
      return Promise.reject(
        new CommandError("transport", `not connected to the Yhtye bridge (${this.target})`),
      );
    }
    const id = this.nextId++;
    const request: WsRequest = { id, cmd };
    return new Promise<ApiResponse>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new CommandError("transport", `no reply to ${cmd.type} within ${this.requestTimeoutMs} ms`));
      }, this.requestTimeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      try {
        socket.send(JSON.stringify(request));
      } catch (e) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(toCommandError(e));
      }
    });
  }

  subscribe(handler: (ev: ApiEvent) => void): () => void {
    return this.events.add(handler);
  }

  onStatus(handler: (status: ConnectionStatus) => void): () => void {
    handler(this.status);
    return this.statuses.add(handler);
  }

  close(): void {
    this.closed = true;
    if (this.retryTimer) clearTimeout(this.retryTimer);
    this.socket?.close();
    this.failPending("the connection was closed");
    this.setStatus({ state: "closed", reason: "closed", retryInMs: null });
  }

  private connect(): void {
    this.setStatus({ state: "connecting" });
    let socket: WebSocketLike;
    try {
      socket = this.createSocket(this.url);
    } catch (e) {
      this.onDisconnect(`cannot connect: ${toCommandError(e).message}`);
      return;
    }
    this.socket = socket;
    socket.onopen = () => {
      this.attempt = 0;
      this.setStatus({ state: "open" });
    };
    socket.onmessage = (ev) => this.onMessage(ev.data);
    socket.onerror = () => {
      // A close event always follows; it carries the reason.
    };
    socket.onclose = (ev) => {
      if (this.socket !== socket) return;
      this.socket = null;
      const why = ev.reason || `code ${ev.code}`;
      this.onDisconnect(`connection to ${this.target} closed (${why})`);
    };
  }

  private onDisconnect(reason: string): void {
    this.failPending(reason);
    if (this.closed) return;
    const delay = this.delays[Math.min(this.attempt, this.delays.length - 1)] ?? 1000;
    this.attempt += 1;
    this.setStatus({ state: "closed", reason, retryInMs: delay });
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      if (!this.closed) this.connect();
    }, delay);
  }

  private onMessage(data: unknown): void {
    const msg = parseIncoming(data);
    if (!msg) return; // not a bridge message; nothing can be matched to it
    if (msg.kind === "event") {
      this.events.emit(msg.event);
      return;
    }
    const p = this.pending.get(msg.id);
    if (!p) return;
    this.pending.delete(msg.id);
    clearTimeout(p.timer);
    if (msg.kind === "ok") p.resolve(msg.ok);
    else p.reject(toCommandError(msg.err));
  }

  private failPending(reason: string): void {
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.reject(new CommandError("transport", reason));
    }
    this.pending.clear();
  }

  private setStatus(status: ConnectionStatus): void {
    this.status = status;
    this.statuses.emit(status);
  }
}
