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
  ChatInfo,
  GitOverview,
  HarnessModels,
  ModelEfforts,
  ProjectInfo,
  RoleSettings,
  UsageReport,
} from "../api/generated";
import { type ConnectionStatus, type Transport, toCommandError } from "../api/transport";
import { isOrchestratorKey, upsertChat } from "./chats";
import { memoryPrefs, type Prefs } from "./prefs";
import {
  applyDurable,
  applyLive,
  applySnapshot,
  clearStreaming,
  newProjectView,
  prependHistory,
  type ProjectView,
  removeChatNow,
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
/** Preference key (per project id): the chat shown when the project is opened again. */
export const chatPrefKey = (project: string): string => `yhtye.chat.${project}`;
const MAX_ERRORS = 5;
/** How often the usage meters are re-read while connected (the core caches for a minute). */
export const USAGE_REFRESH_MS = 5 * 60_000;
/** How often the core is pinged while connected (the sidebar's host footer). */
export const PING_INTERVAL_MS = 3_000;
/** A ping without an answer after this long counts as missed. */
export const PING_TIMEOUT_MS = 5_000;

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

/** The machine the core runs on, as its last answer to a ping told. */
export interface HostView {
  /** Its host name (`null` until the first answer). */
  name: string | null;
  /** When the core last answered a ping (ms since the epoch). */
  lastPingAt: number | null;
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
  host: HostView;
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
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private pinging = false;
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
      host: { name: null, lastPingAt: null },
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
    this.stopPinging();
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
    const view = this.state.project;
    if (!view?.selectedChat) return false;
    const chat = view.selectedChat;
    const project = view.info.id;
    this.set({ busy: { ...this.state.busy, sending: true } });
    try {
      const r = await this.run({ type: "send_user_message", project, chat, text }, "accepted");
      return r !== null;
    } finally {
      this.set({ busy: { ...this.state.busy, sending: false } });
    }
  }

  async cancelTurn(): Promise<void> {
    const view = this.state.project;
    if (!view?.selectedChat) return;
    await this.run({ type: "cancel_orchestrator_turn", project: view.info.id, chat: view.selectedChat }, "accepted");
  }

  /** Shows another chat (no process is started); remembered for the project. */
  selectChat(chat: string): void {
    const view = this.state.project;
    if (!view || view.selectedChat === chat || !view.chats.some((c) => c.id === chat)) return;
    this.prefs.set(chatPrefKey(view.info.id), chat);
    const { [chat]: _read, ...unread } = view.unread;
    this.updateProject((v) => ({ ...v, selectedChat: chat, unread }));
  }

  /**
   * A new chat (no orchestrator yet) in a worktree, or in the worktree of a
   * branch (Yhtye makes one if it is checked out nowhere); it becomes the
   * selected one. Failures are shown as errors.
   */
  async createChat(target: { worktree: string } | { branch: string }): Promise<boolean> {
    const project = this.state.project?.info.id;
    if (!project) return false;
    const r = await this.run({ type: "create_chat", project, ...target }, "chat");
    if (!r) return false;
    // A branch may have got a new worktree: list it before showing the chat.
    if ("branch" in target) await this.refreshGit();
    this.adoptChat(project, r.chat);
    return true;
  }

  /**
   * A new branch in Yhtye's worktree with its first chat (selected). `from`
   * defaults to the main clone's HEAD. Rejects with the core's error (the
   * form shows it next to the input).
   */
  async createBranch(name: string, from?: string): Promise<void> {
    const project = this.state.project?.info.id;
    if (!project) return;
    const r = await this.invoke({ type: "create_branch", project, name, from }, "chat");
    // Git first: a chat shown before the overview lists its worktree reads as "not found".
    await this.refreshGit();
    this.adoptChat(project, r.chat);
  }

  /** Lists a chat the core just created (its event may not have arrived yet) and selects it. */
  private adoptChat(project: string, chat: ChatInfo): void {
    if (this.state.project?.info.id !== project) return;
    this.updateProject((v) => ({ ...v, chats: upsertChat(v.chats, chat) }));
    this.selectChat(chat.id);
  }

  /**
   * The user's own title for a chat. Rejects with the core's error (an empty
   * or too long title, an unknown chat) for the editor to show.
   */
  async renameChat(chat: string, title: string): Promise<void> {
    const project = this.state.project?.info.id;
    if (!project) return;
    const r = await this.invoke({ type: "rename_chat", project, chat, title }, "chat");
    if (this.state.project?.info.id !== project) return;
    // The core's `chat_titled` follows; showing the answer now avoids a flash of the old title.
    this.updateProject((v) => ({ ...v, chats: v.chats.map((c) => (c.id === chat ? { ...c, title: r.chat.title } : c)) }));
  }

  /**
   * Deletes a chat (its worktree stays). Rejects with the core's refusal
   * (its orchestrator is working, a group is unfinished) for the confirmation
   * to show. Another chat is shown if this one was.
   */
  async deleteChat(chat: string): Promise<void> {
    const project = this.state.project?.info.id;
    if (!project) return;
    await this.invoke({ type: "delete_chat", project, chat }, "accepted");
    if (this.state.project?.info.id !== project) return;
    this.updateProject((v) => removeChatNow(v, chat));
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
      const remembered = this.prefs.get(chatPrefKey(info.id));
      this.updateProject((v) => applySnapshot(v, snap.snapshot, this.historyWindow, remembered));
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
    if ((!ev.live && changesGit(ev)) || endsOrchestratorTurn(ev)) this.scheduleGitRefresh();
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
      this.stopPinging();
      return;
    }
    this.startUsagePolling();
    this.startPinging();
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

  private startPinging(): void {
    void this.ping();
    if (this.pingTimer) return;
    this.pingTimer = setInterval(() => void this.ping(), PING_INTERVAL_MS);
  }

  private stopPinging(): void {
    if (this.pingTimer) clearInterval(this.pingTimer);
    this.pingTimer = null;
  }

  /**
   * One liveness check of the core. A miss is not an error to show: the footer
   * counts the time since the last answer.
   */
  private async ping(): Promise<void> {
    if (this.pinging) return;
    this.pinging = true;
    try {
      const r = await withTimeout(this.invoke({ type: "ping" }, "pong"), PING_TIMEOUT_MS);
      this.set({ host: { name: r.host, lastPingAt: Date.now() } });
    } catch {
      // Missed; the next ping tries again.
    } finally {
      this.pinging = false;
    }
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

  /** The efforts of one model whose efforts the model list did not include (Stage 7d). */
  async listModelEfforts(harness: string, model: string): Promise<ModelEfforts> {
    const r = await this.invoke({ type: "list_model_efforts", harness, model }, "model_efforts");
    return r.efforts;
  }

  // ---- secret environment variables (Stage 7e) ------------------------------
  // Only names come back; a value goes to the core once and is not kept here.
  // Like the agent settings, failures reject for the section to show.

  /** The registered names, sorted. */
  async listSecretEnv(): Promise<string[]> {
    return (await this.invoke({ type: "list_secret_env" }, "secret_env")).names;
  }

  /** Stores `value` in the OS keyring under `name` (replacing an earlier one). */
  async setSecretEnv(name: string, value: string): Promise<string[]> {
    return (await this.invoke({ type: "set_secret_env", name, value }, "secret_env")).names;
  }

  async deleteSecretEnv(name: string): Promise<string[]> {
    return (await this.invoke({ type: "delete_secret_env", name }, "secret_env")).names;
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

/**
 * An orchestrator finished a turn: it may have renamed or switched a branch
 * itself (Stage 8e: the tree shows what the worktrees have checked out now).
 */
function endsOrchestratorTurn(ev: ApiEvent): boolean {
  return ev.body.type === "agent" && ev.body.event.type === "turn_ended" && isOrchestratorKey(ev.body.session);
}

/** Durable domain events after which branches or commits may have changed. */
function changesGit(ev: ApiEvent): boolean {
  if (ev.body.type !== "domain") return false;
  switch (ev.body.event.type) {
    case "chat_created":
    case "group_created":
    case "group_base_changed":
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

/** `promise`, or a rejection if it takes longer than `ms`. */
function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`no answer within ${ms} ms`)), ms);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

function upsertProject(list: ProjectInfo[], p: ProjectInfo): ProjectInfo[] {
  return list.some((x) => x.id === p.id) ? list.map((x) => (x.id === p.id ? p : x)) : [...list, p];
}
