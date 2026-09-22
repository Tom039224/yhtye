// The single application store: connection status, known projects, the open
// project's view, and user-visible errors. It owns the sync with the core:
//
// - load: subscribe (events are buffered) → GetSnapshot → ListEvents pages from
//   the start of the log (history) → drain the buffer.
// - steady state: durable events must arrive as `cursor + 1`; older ones are
//   duplicates and skipped; a gap (or a live event from after an unseen durable
//   event) triggers a catch-up with ListEvents from `cursor`.
// - reconnect: re-open the project (idempotent; the core may have restarted)
//   and catch up from `cursor`. Streamed text of the dead connection is dropped.

import type { ApiCommand, ApiEvent, ApiResponse, ProjectInfo } from "../api/generated";
import { type ConnectionStatus, type Transport, toCommandError } from "../api/transport";
import {
  applyDurable,
  applyLive,
  applySnapshot,
  clearStreaming,
  newProjectView,
  type ProjectView,
} from "./project";

export const EVENT_PAGE = 500;
const MAX_ERRORS = 5;

export interface AppError {
  id: number;
  message: string;
}

export interface AppState {
  connection: ConnectionStatus;
  transport: { kind: Transport["kind"]; target: string };
  projects: ProjectInfo[];
  project: ProjectView | null;
  errors: AppError[];
  busy: { opening: boolean; sending: boolean };
}

type Expect<T extends ApiResponse["type"]> = Extract<ApiResponse, { type: T }>;

export class AppStore {
  private state: AppState;
  private readonly listeners = new Set<() => void>();
  private readonly unsubscribe: (() => void)[] = [];
  private buffer: ApiEvent[] = [];
  private syncing = false;
  private everOpen = false;
  private loadToken = 0;
  private nextErrorId = 1;

  constructor(private readonly transport: Transport) {
    this.state = {
      connection: { state: "connecting" },
      transport: { kind: transport.kind, target: transport.target },
      projects: [],
      project: null,
      errors: [],
      busy: { opening: false, sending: false },
    };
  }

  start(): void {
    this.unsubscribe.push(this.transport.subscribe((ev) => this.onEvent(ev)));
    this.unsubscribe.push(this.transport.onStatus((s) => this.onStatus(s)));
  }

  stop(): void {
    for (const un of this.unsubscribe.splice(0)) un();
  }

  getState = (): AppState => this.state;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  // ---- commands -------------------------------------------------------------

  async refreshProjects(): Promise<void> {
    const r = await this.run({ type: "list_projects" }, "projects");
    if (r) this.set({ projects: r.projects });
  }

  async openProject(path: string): Promise<void> {
    this.set({ busy: { ...this.state.busy, opening: true } });
    try {
      const r = await this.run({ type: "open_project", path }, "project");
      if (!r) return;
      this.set({ projects: upsertProject(this.state.projects, r.project) });
      await this.load(r.project);
    } finally {
      this.set({ busy: { ...this.state.busy, opening: false } });
    }
  }

  /** Returns whether the message was accepted (the composer keeps it otherwise). */
  async sendMessage(text: string): Promise<boolean> {
    const project = this.state.project?.info.id;
    if (!project) return false;
    this.set({ busy: { ...this.state.busy, sending: true } });
    try {
      const r = await this.run({ type: "send_user_message", project, text }, "accepted");
      return r !== null;
    } finally {
      this.set({ busy: { ...this.state.busy, sending: false } });
    }
  }

  async cancelTurn(): Promise<void> {
    const project = this.state.project?.info.id;
    if (project) await this.run({ type: "cancel_orchestrator_turn", project }, "accepted");
  }

  async cancelTask(task: string): Promise<void> {
    const project = this.state.project?.info.id;
    if (project) await this.run({ type: "cancel_task", project, task }, "accepted");
  }

  async cancelGroup(group: string): Promise<void> {
    const project = this.state.project?.info.id;
    if (project) await this.run({ type: "cancel_group", project, group }, "accepted");
  }

  dismissError(id: number): void {
    this.set({ errors: this.state.errors.filter((e) => e.id !== id) });
  }

  // ---- sync -----------------------------------------------------------------

  private async load(info: ProjectInfo): Promise<void> {
    const token = ++this.loadToken;
    this.buffer = [];
    this.syncing = true;
    this.set({ project: newProjectView(info) });
    try {
      const snap = await this.invoke({ type: "get_snapshot", project: info.id }, "snapshot");
      if (token !== this.loadToken) return;
      this.updateProject((v) => applySnapshot(v, snap.snapshot));
      await this.catchUp(token);
      if (token !== this.loadToken) return;
      this.updateProject((v) => ({ ...v, phase: "ready" }));
    } catch (e) {
      if (token !== this.loadToken) return;
      const message = `could not load ${info.name}: ${toCommandError(e).message}`;
      this.syncing = false;
      this.updateProject((v) => ({ ...v, phase: "error", loadError: message }));
      this.pushError(message);
    }
  }

  /** Applies every stored event after `cursor`, then the buffered stream. */
  private async catchUp(token: number): Promise<void> {
    this.syncing = true;
    try {
      for (;;) {
        const project = this.state.project;
        if (!project || token !== this.loadToken) return;
        const cmd: ApiCommand = {
          type: "list_events",
          project: project.info.id,
          after_seq: project.cursor,
          limit: EVENT_PAGE,
        };
        const page = await this.invoke(cmd, "events");
        if (token !== this.loadToken) return;
        this.updateProject((v) =>
          page.events.reduce((acc, ev) => (ev.seq === acc.cursor + 1 ? applyDurable(acc, ev) : acc), v),
        );
        if (!page.more) break;
      }
    } finally {
      if (token === this.loadToken) this.syncing = false;
    }
    const buffered = this.buffer;
    this.buffer = [];
    for (const ev of buffered) this.onEvent(ev);
  }

  private requestCatchUp(): void {
    if (this.syncing) return;
    const token = this.loadToken;
    this.catchUp(token).catch((e: unknown) => {
      this.pushError(`catching up with the event log failed: ${toCommandError(e).message}`);
    });
  }

  private onEvent(ev: ApiEvent): void {
    const view = this.state.project;
    if (!view || ev.project !== view.info.id || view.phase === "error") return;
    if (this.syncing) {
      this.buffer.push(ev);
      return;
    }
    if (ev.live) {
      // Older than what is applied: its block may already be complete (replayed
      // from the buffer after a catch-up); showing it again would duplicate text.
      if (ev.seq < view.cursor) return;
      if (ev.seq > view.cursor) this.requestCatchUp();
      this.updateProject((v) => applyLive(v, ev));
      return;
    }
    if (ev.seq <= view.cursor) return; // already applied (from the log or a replay)
    if (ev.seq === view.cursor + 1) {
      this.updateProject((v) => applyDurable(v, ev));
      return;
    }
    this.buffer.push(ev);
    this.requestCatchUp();
  }

  private onStatus(status: ConnectionStatus): void {
    const wasOpen = this.state.connection.state === "open";
    this.set({ connection: status });
    if (status.state !== "open") {
      if (wasOpen) this.updateProject(clearStreaming);
      return;
    }
    const reconnect = this.everOpen;
    this.everOpen = true;
    void this.refreshProjects();
    const view = this.state.project;
    if (reconnect && view) void this.resync(view.info);
  }

  /** After a reconnect: make sure the project is open, then catch up. */
  private async resync(info: ProjectInfo): Promise<void> {
    if (this.state.project?.phase !== "ready") {
      await this.openProject(info.path);
      return;
    }
    const r = await this.run({ type: "open_project", path: info.path }, "project");
    if (!r || this.state.project?.info.id !== info.id) return;
    this.updateProject((v) => ({ ...v, info: r.project }));
    this.requestCatchUp();
  }

  // ---- helpers --------------------------------------------------------------

  /** Runs a command; failures become visible errors and return `null`. */
  private async run<T extends ApiResponse["type"]>(cmd: ApiCommand, expect: T): Promise<Expect<T> | null> {
    try {
      return await this.invoke(cmd, expect);
    } catch (e) {
      this.pushError(`${cmd.type} failed: ${toCommandError(e).message}`);
      return null;
    }
  }

  private async invoke<T extends ApiResponse["type"]>(cmd: ApiCommand, expect: T): Promise<Expect<T>> {
    const r = await this.transport.invoke(cmd);
    if (r.type !== expect) {
      throw new Error(`unexpected reply to ${cmd.type}: ${r.type}`);
    }
    return r as Expect<T>;
  }

  private pushError(message: string): void {
    const error = { id: this.nextErrorId++, message };
    this.set({ errors: [...this.state.errors, error].slice(-MAX_ERRORS) });
  }

  private updateProject(f: (v: ProjectView) => ProjectView): void {
    const view = this.state.project;
    if (!view) return;
    const next = f(view);
    if (next !== view) this.set({ project: next });
  }

  private set(patch: Partial<AppState>): void {
    this.state = { ...this.state, ...patch };
    for (const l of [...this.listeners]) l();
  }
}

function upsertProject(list: ProjectInfo[], p: ProjectInfo): ProjectInfo[] {
  return list.some((x) => x.id === p.id) ? list.map((x) => (x.id === p.id ? p : x)) : [...list, p];
}
