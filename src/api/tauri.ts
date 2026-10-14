// Tauri IPC transport (core-design.md §9): one command `yhtye_command(host, cmd)`
// and the event `yhtye://event`, which carries `{host, event}`. `host` is `null`
// for this app's own core and the `server_id` of a remote host (another Yhtye
// this app is a client of; the shell holds the WebSocket to it). A transport
// speaks to one host: it sends its commands there and takes only its events.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { ApiCommand, ApiEvent, ApiResponse } from "./generated";
import { type RemoteHostInfo, linkStatus } from "./remoteHosts";
import { type ConnectionStatus, Listeners, type Transport, toCommandError } from "./transport";

export const TAURI_COMMAND = "yhtye_command";
export const TAURI_EVENT = "yhtye://event";

/** The payload of {@link TAURI_EVENT}: an event and whose it is. */
export interface HostEvent {
  /** `null`: the local core. Else the `server_id` of the remote host. */
  host: string | null;
  event: ApiEvent;
}

export class TauriTransport implements Transport {
  readonly kind = "tauri";
  readonly target: string;
  private readonly events = new Listeners<ApiEvent>();
  private readonly statuses = new Listeners<ConnectionStatus>();
  private status: ConnectionStatus = { state: "connecting" };
  private unlisten: UnlistenFn | null = null;
  private listening = false;
  private failure: string | null = null;
  /** A remote host's own connection, as the shell reports it (`applyHost`). */
  private link: ConnectionStatus = { state: "connecting" };
  /** The connection (`session`) of the host that `link` says is open. */
  private session: number | null = null;
  private closed = false;

  /** `hostId`: the remote host this speaks to (`null`: the local core). */
  constructor(readonly hostId: string | null = null) {
    this.target = hostId === null ? "Tauri IPC" : `Tauri IPC → ${hostId}`;
    listen<HostEvent>(TAURI_EVENT, (e) => {
      if (e.payload.host === this.hostId) this.events.emit(e.payload.event);
    })
      .then((un) => {
        if (this.closed) {
          un();
          return;
        }
        this.unlisten = un;
        this.listening = true;
        this.refreshStatus();
      })
      .catch((e: unknown) => {
        this.failure = `cannot listen to ${TAURI_EVENT}: ${toCommandError(e).message}`;
        this.refreshStatus();
      });
  }

  async invoke(cmd: ApiCommand): Promise<ApiResponse> {
    try {
      return await invoke<ApiResponse>(TAURI_COMMAND, { host: this.hostId, cmd });
    } catch (e) {
      throw toCommandError(e);
    }
  }

  subscribe(handler: (ev: ApiEvent) => void): () => void {
    return this.events.add(handler);
  }

  onStatus(handler: (status: ConnectionStatus) => void): () => void {
    handler(this.status);
    return this.statuses.add(handler);
  }

  /**
   * What the shell says of the connection to this transport's remote host. The
   * transport is open while that is, and once the events can be heard. A new
   * connection that the shell did not report a break before (the host's events
   * of the time in between are lost) shows as connecting for a moment, so the
   * store reads its state again.
   */
  applyHost(info: RemoteHostInfo): void {
    if (this.hostId === null) return;
    const connection = info.status.connection;
    const next = linkStatus(connection);
    const session = connection.state === "connected" ? connection.session : null;
    if (session !== null && this.session !== null && session !== this.session && this.status.state === "open") {
      this.link = { state: "connecting" };
      this.refreshStatus();
    }
    if (session !== null) this.session = session;
    this.link = next;
    this.refreshStatus();
  }

  close(): void {
    this.closed = true;
    this.unlisten?.();
    this.setStatus({ state: "closed", reason: "closed", retryInMs: null });
  }

  private refreshStatus(): void {
    if (this.closed) return;
    this.setStatus(this.currentStatus());
  }

  private currentStatus(): ConnectionStatus {
    if (this.failure !== null) return { state: "closed", reason: this.failure, retryInMs: null };
    if (!this.listening) return { state: "connecting" };
    return this.hostId === null ? { state: "open" } : this.link;
  }

  private setStatus(status: ConnectionStatus): void {
    if (sameStatus(this.status, status)) return;
    this.status = status;
    this.statuses.emit(status);
  }
}

function sameStatus(a: ConnectionStatus, b: ConnectionStatus): boolean {
  if (a.state !== b.state) return false;
  if (a.state === "closed" && b.state === "closed") return a.reason === b.reason && a.retryInMs === b.retryInMs;
  return true;
}
