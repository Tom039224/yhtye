// Tauri IPC transport (core-design.md §9): one command `yhtye_command(cmd)` and
// the event `yhtye://event`. The Rust side is wired in Stage 5.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { ApiCommand, ApiEvent, ApiResponse } from "./generated";
import { type ConnectionStatus, Listeners, type Transport, toCommandError } from "./transport";

export const TAURI_COMMAND = "yhtye_command";
export const TAURI_EVENT = "yhtye://event";

export class TauriTransport implements Transport {
  readonly kind = "tauri";
  readonly target = "Tauri IPC";
  private readonly events = new Listeners<ApiEvent>();
  private readonly statuses = new Listeners<ConnectionStatus>();
  private status: ConnectionStatus = { state: "connecting" };
  private unlisten: UnlistenFn | null = null;
  private closed = false;

  constructor() {
    listen<ApiEvent>(TAURI_EVENT, (e) => this.events.emit(e.payload))
      .then((un) => {
        if (this.closed) {
          un();
          return;
        }
        this.unlisten = un;
        this.setStatus({ state: "open" });
      })
      .catch((e: unknown) => {
        const reason = `cannot listen to ${TAURI_EVENT}: ${toCommandError(e).message}`;
        this.setStatus({ state: "closed", reason, retryInMs: null });
      });
  }

  async invoke(cmd: ApiCommand): Promise<ApiResponse> {
    try {
      return await invoke<ApiResponse>(TAURI_COMMAND, { cmd });
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

  close(): void {
    this.closed = true;
    this.unlisten?.();
    this.setStatus({ state: "closed", reason: "closed", retryInMs: null });
  }

  private setStatus(status: ConnectionStatus): void {
    this.status = status;
    this.statuses.emit(status);
  }
}
