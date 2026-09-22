// The one interface the UI talks to the core through (core-design.md §11).
// Implementations: TauriTransport (app), WsTransport (dev bridge in a browser),
// MemoryTransport (tests only, src/test/).

import type { ApiCommand, ApiErrorCode, ApiEvent, ApiResponse } from "./generated";

export type ConnectionStatus =
  | { state: "connecting" }
  | { state: "open" }
  | { state: "closed"; reason: string; retryInMs: number | null };

export type TransportKind = "tauri" | "websocket" | "memory";

export interface Transport {
  readonly kind: TransportKind;
  /** Human-readable target (bridge URL, "Tauri IPC", ...). */
  readonly target: string;
  /** Runs a command. Rejects with a {@link CommandError}. */
  invoke(cmd: ApiCommand): Promise<ApiResponse>;
  /** Receives every event pushed by the core. Returns an unsubscribe function. */
  subscribe(handler: (ev: ApiEvent) => void): () => void;
  /** Receives connection status changes, starting with the current status. */
  onStatus(handler: (status: ConnectionStatus) => void): () => void;
  close(): void;
}

/** Why a command failed: an error of the core, or of the connection itself. */
export class CommandError extends Error {
  readonly code: ApiErrorCode | "transport";

  constructor(code: ApiErrorCode | "transport", message: string) {
    super(message);
    this.name = "CommandError";
    this.code = code;
  }
}

const API_ERROR_CODES: readonly string[] = [
  "invalid_argument",
  "not_found",
  "invalid_state",
  "conflict",
  "forbidden",
  "unavailable",
  "internal",
] satisfies ApiErrorCode[];

function isApiErrorCode(code: unknown): code is ApiErrorCode {
  return typeof code === "string" && API_ERROR_CODES.includes(code);
}

/** Normalizes anything a transport rejected with into a {@link CommandError}. */
export function toCommandError(e: unknown): CommandError {
  if (e instanceof CommandError) return e;
  if (typeof e === "object" && e !== null && "code" in e && "message" in e) {
    const { code, message } = e;
    if (isApiErrorCode(code) && typeof message === "string") {
      return new CommandError(code, message);
    }
  }
  if (e instanceof Error) return new CommandError("transport", e.message);
  return new CommandError("transport", String(e));
}

/** Small listener set shared by the transports. */
export class Listeners<T> {
  private readonly set = new Set<(value: T) => void>();

  add(handler: (value: T) => void): () => void {
    this.set.add(handler);
    return () => {
      this.set.delete(handler);
    };
  }

  emit(value: T): void {
    for (const handler of [...this.set]) handler(value);
  }
}
