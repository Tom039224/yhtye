// Test-only transport and a small in-memory core that serves a recorded or
// constructed event log. Never used on app runtime paths.

import type {
  ApiCommand,
  ApiEvent,
  ApiResponse,
  LoggedEvent,
  ProjectInfo,
  Snapshot,
} from "../api/generated";
import { CommandError, type ConnectionStatus, Listeners, type Transport, toCommandError } from "../api/transport";

type Handler = (cmd: ApiCommand) => ApiResponse | Promise<ApiResponse>;

export class MemoryTransport implements Transport {
  readonly kind = "memory";
  readonly target = "memory";
  readonly calls: ApiCommand[] = [];
  handler: Handler = (cmd) => {
    throw new CommandError("internal", `no handler for ${cmd.type}`);
  };
  private readonly events = new Listeners<ApiEvent>();
  private readonly statuses = new Listeners<ConnectionStatus>();
  private status: ConnectionStatus;

  constructor(status: ConnectionStatus = { state: "open" }) {
    this.status = status;
  }

  async invoke(cmd: ApiCommand): Promise<ApiResponse> {
    this.calls.push(cmd);
    try {
      return await this.handler(cmd);
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

  close(): void {}

  emit(ev: ApiEvent): void {
    this.events.emit(ev);
  }

  setStatus(status: ConnectionStatus): void {
    this.status = status;
    this.statuses.emit(status);
  }

  callsOf<T extends ApiCommand["type"]>(type: T): Extract<ApiCommand, { type: T }>[] {
    return this.calls.filter((c): c is Extract<ApiCommand, { type: T }> => c.type === type);
  }
}

/** Serves `list_events` / `get_snapshot` / `open_project` from an event log. */
export class FakeCore {
  log: ApiEvent[] = [];
  snapshot: Snapshot;
  info: ProjectInfo;
  /** Commands that should fail, by type. */
  failures = new Map<ApiCommand["type"], CommandError>();
  /** Largest page the fake returns (whatever the client asks for). */
  pageCap = Number.MAX_SAFE_INTEGER;
  /** Called before answering a command (to interleave pushed events). */
  before: ((cmd: ApiCommand) => void | Promise<void>) | null = null;

  constructor(info: ProjectInfo, snapshot: Snapshot) {
    this.info = info;
    this.snapshot = snapshot;
  }

  attach(t: MemoryTransport): void {
    t.handler = async (cmd) => {
      await this.before?.(cmd);
      return this.handle(cmd);
    };
  }

  handle(cmd: ApiCommand): ApiResponse {
    const failure = this.failures.get(cmd.type);
    if (failure) throw failure;
    switch (cmd.type) {
      case "list_projects":
        return { type: "projects", projects: [this.info] };
      case "open_project":
        return { type: "project", project: { ...this.info, open: true } };
      case "get_snapshot":
        return { type: "snapshot", snapshot: this.snapshot };
      case "list_events": {
        const limit = Math.min(cmd.limit ?? 500, this.pageCap);
        const after = this.log.filter((e) => !e.live && e.seq > cmd.after_seq);
        const events: LoggedEvent[] = after.slice(0, limit).map((e) => ({ ...e, live: false }));
        return { type: "events", events, more: after.length > limit };
      }
      default:
        return { type: "accepted" };
    }
  }
}
