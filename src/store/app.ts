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

import type {
  AgentRole,
  AgentSettingsView,
  ApiCommand,
  ApiEvent,
  ApiResponse,
  GitOverview,
  HarnessModels,
  ProjectInfo,
  RoleSettings,
  UsageReport,
} from "../api/generated";
import { type ConnectionStatus, type Transport, toCommandError } from "../api/transport";
import { memoryPrefs, type Prefs } from "./prefs";
import {
  applyDurable,
  applyLive,
  applySnapshot,
  clearStreaming,
  newProjectView,
  prependHistory,
  type ProjectView,
} from "./project";

export const EVENT_PAGE = 500;
/** Events read for the transcripts when a project loads (older ones on demand). */
export const HISTORY_WINDOW = 400;
/** Events per "load older history" page. */
export const HISTORY_PAGE = 400;
/** Commits of the git panel. */
export const GIT_COMMITS = 120;
/** Delay before re-reading git after a domain change (changes come in bursts). */
export const GIT_REFRESH_MS = 400;
/** Preference key: path of the project opened last (reopened after a reload). */
export const LAST_PROJECT_KEY = "yhtye.lastProject";
const MAX_ERRORS = 5;
/** How often the usage meters are re-read while connected (the core caches for a minute). */
export const USAGE_REFRESH_MS = 5 * 60_000;

/** The git panel's data for the open project. */
export interface GitView {
  project: string;
  overview: GitOverview | null;
  /** Why the last read failed (shown in the panel). */
  error: string | null;
  loading: boolean;
}

/** Subscription usage for the status bar (read from the harness by the core). */
export interface UsageView {
  report: UsageReport | null;
  /** Why the last read failed (the meters then stay empty). */
  error: string | null;
  loading: boolean;
}

export interface AppStoreOptions {
  /** Where the last project is remembered (default: in memory only). */
  prefs?: Prefs;
  historyWindow?: number;
  historyPage?: number;
}

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
  git: GitView | null;
  usage: UsageView;
  /** When the last event of any project arrived (ms since the epoch). */
  lastEventAt: number | null;
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
  private gitTimer: ReturnType<typeof setTimeout> | null = null;
  private gitToken = 0;
  private usageTimer: ReturnType<typeof setInterval> | null = null;
  private readonly prefs: Prefs;
  private readonly historyWindow: number;
  private readonly historyPage: number;

  constructor(
    private readonly transport: Transport,
    options: AppStoreOptions = {},
  ) {
    this.prefs = options.prefs ?? memoryPrefs();
    this.historyWindow = options.historyWindow ?? HISTORY_WINDOW;
    this.historyPage = options.historyPage ?? HISTORY_PAGE;
    this.state = {
      connection: { state: "connecting" },
      transport: { kind: transport.kind, target: transport.target },
      projects: [],
      project: null,
      errors: [],
      busy: { opening: false, sending: false },
      git: null,
      usage: { report: null, error: null, loading: false },
      lastEventAt: null,
    };
  }

  start(): void {
    this.unsubscribe.push(this.transport.subscribe((ev) => this.onEvent(ev)));
    this.unsubscribe.push(this.transport.onStatus((s) => this.onStatus(s)));
  }

  stop(): void {
    for (const un of this.unsubscribe.splice(0)) un();
    if (this.gitTimer) clearTimeout(this.gitTimer);
    this.gitTimer = null;
    this.stopUsagePolling();
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

  async retryGroupMerge(group: string): Promise<void> {
    const project = this.state.project?.info.id;
    if (project) await this.run({ type: "retry_group_merge", project, group }, "accepted");
  }

  /** Loads the page of history before what is shown (lazy history, newest first). */
  async loadOlderHistory(): Promise<void> {
    const view = this.state.project;
    if (!view || view.phase !== "ready" || view.loadingOlder || view.historyStart <= 1) return;
    const token = this.loadToken;
    const from = Math.max(1, view.historyStart - this.historyPage);
    this.updateProject((v) => ({ ...v, loadingOlder: true }));
    try {
      const events: ApiEvent[] = [];
      let after = from - 1;
      while (after < view.historyStart - 1) {
        const cmd: ApiCommand = {
          type: "list_events",
          project: view.info.id,
          after_seq: after,
          limit: view.historyStart - 1 - after,
        };
        const page = await this.invoke(cmd, "events");
        if (token !== this.loadToken) return;
        if (page.events.length === 0) break;
        events.push(...page.events);
        after = page.events[page.events.length - 1].seq;
      }
      this.updateProject((v) => ({ ...prependHistory(v, events, from), loadingOlder: false }));
    } catch (e) {
      if (token !== this.loadToken) return;
      this.updateProject((v) => ({ ...v, loadingOlder: false }));
      this.pushError(`loading older history failed: ${toCommandError(e).message}`);
    }
  }

  /** Re-reads the open project's branches and commit graph. */
  async refreshGit(): Promise<void> {
    const view = this.state.project;
    if (!view) return;
    const project = view.info.id;
    const token = ++this.gitToken;
    const prev = this.state.git?.project === project ? this.state.git : null;
    this.set({ git: { project, overview: prev?.overview ?? null, error: null, loading: true } });
    try {
      const r = await this.invoke({ type: "get_git_overview", project, limit: GIT_COMMITS }, "git_overview");
      if (token !== this.gitToken) return;
      this.set({ git: { project, overview: r.git, error: null, loading: false } });
    } catch (e) {
      if (token !== this.gitToken) return;
      const error = toCommandError(e).message;
      this.set({ git: { project, overview: prev?.overview ?? null, error, loading: false } });
    }
  }

  /**
   * Re-reads the subscription usage. `refresh` asks the core to query the
   * harness again (a click on the meters); otherwise its cached value may do.
   * A failure keeps no values: the meters show "—" with the reason.
   */
  async refreshUsage(refresh = false): Promise<void> {
    if (this.state.usage.loading) return;
    this.set({ usage: { ...this.state.usage, loading: true } });
    try {
      const r = await this.invoke({ type: "get_usage", refresh }, "usage");
      this.set({ usage: { report: r.usage, error: null, loading: false } });
    } catch (e) {
      this.set({ usage: { report: null, error: toCommandError(e).message, loading: false } });
    }
  }

  dismissError(id: number): void {
    this.set({ errors: this.state.errors.filter((e) => e.id !== id) });
  }

  // ---- sync -----------------------------------------------------------------

  private async load(info: ProjectInfo): Promise<void> {
    const token = ++this.loadToken;
    this.buffer = [];
    this.syncing = true;
    const keepGit = this.state.git?.project === info.id ? this.state.git : null;
    this.set({ project: newProjectView(info), git: keepGit });
    this.prefs.set(LAST_PROJECT_KEY, info.path);
    void this.refreshGit();
    try {
      const snap = await this.invoke({ type: "get_snapshot", project: info.id }, "snapshot");
      if (token !== this.loadToken) return;
      this.updateProject((v) => applySnapshot(v, snap.snapshot, this.historyWindow));
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
    this.state = { ...this.state, lastEventAt: Date.now() };
    const view = this.state.project;
    if (!view || ev.project !== view.info.id || view.phase === "error") {
      this.notify();
      return;
    }
    if (!ev.live && changesGit(ev)) this.scheduleGitRefresh();
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
      this.stopUsagePolling();
      return;
    }
    this.startUsagePolling();
    const reconnect = this.everOpen;
    this.everOpen = true;
    const view = this.state.project;
    if (reconnect && view) {
      void this.refreshProjects();
      void this.resync(view.info);
      void this.refreshGit();
    } else {
      void this.reopenLast();
    }
  }

  /** After a (re)load of the page: open the project that was open last. */
  private async reopenLast(): Promise<void> {
    await this.refreshProjects();
    const last = this.prefs.get(LAST_PROJECT_KEY);
    if (!last || this.state.project || this.state.busy.opening) return;
    if (this.state.projects.some((p) => p.path === last)) await this.openProject(last);
  }

  private startUsagePolling(): void {
    void this.refreshUsage();
    if (this.usageTimer) return;
    this.usageTimer = setInterval(() => void this.refreshUsage(), USAGE_REFRESH_MS);
  }

  private stopUsagePolling(): void {
    if (this.usageTimer) clearInterval(this.usageTimer);
    this.usageTimer = null;
  }

  private scheduleGitRefresh(): void {
    if (this.gitTimer) return;
    this.gitTimer = setTimeout(() => {
      this.gitTimer = null;
      void this.refreshGit();
    }, GIT_REFRESH_MS);
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

  // ---- agent settings (Stage 7b) --------------------------------------------
  // Read and written by the settings panel, which shows failures itself
  // (these reject with a CommandError instead of pushing an app error).

  /** The harness × model settings, globally (`null`) or for a project. */
  async getAgentSettings(project: string | null): Promise<AgentSettingsView> {
    const r = await this.invoke({ type: "get_agent_settings", project: project ?? undefined }, "agent_settings");
    return r.settings;
  }

  /** Replaces (or with `null` removes) one role of the global / a project's layer. */
  async setAgentSettings(project: string | null, role: AgentRole, settings: RoleSettings | null): Promise<AgentSettingsView> {
    const cmd: ApiCommand = { type: "set_agent_settings", project: project ?? undefined, role, settings };
    const r = await this.invoke(cmd, "agent_settings");
    return r.settings;
  }

  /** The models a harness offers (read from it by the core; cached there). */
  async listHarnessModels(harness: string, refresh = false): Promise<HarnessModels> {
    const r = await this.invoke({ type: "list_harness_models", harness, refresh }, "harness_models");
    return r.models;
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
    this.notify();
  }

  private notify(): void {
    for (const l of [...this.listeners]) l();
  }
}

/** Durable domain events after which branches or commits may have changed. */
function changesGit(ev: ApiEvent): boolean {
  if (ev.body.type !== "domain") return false;
  switch (ev.body.event.type) {
    case "group_created":
    case "group_merge_finished":
    case "group_cancelled":
    case "workspace_ready":
    case "task_status_changed":
    case "task_cancelled":
    case "step_completed":
      return true;
    default:
      return false;
  }
}

function upsertProject(list: ProjectInfo[], p: ProjectInfo): ProjectInfo[] {
  return list.some((x) => x.id === p.id) ? list.map((x) => (x.id === p.id ? p : x)) : [...list, p];
}
