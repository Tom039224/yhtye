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
// - other clients (T-65): app-wide events (empty `project`) say that the project
//   list or the settings changed, or that events were dropped for this client
//   (`resync`: catch up like after a reconnect).
// - clients (T-73): the PC and the paired devices for the host footer, read on
//   every (re)connect and resync and replaced by each `clients_changed`.
// - hosts (T-97): a store speaks to one core, through its transport: this app's
//   own, or another Yhtye it is a client of (`store/hosts.ts` makes one store per
//   host). Project ids repeat across hosts (they come from directory names), so the
//   keys this store saves under and the keys the UI makes from a project id carry
//   the host's `keyScope` ("" for the local core, "host:<id>:" for a remote one).

import type { ApiCommand, ApiEvent, ApiEventBody, ApiResponse, BranchStatuses, ChatInfo, ClientDevice, DirectoryListing, GitOverview, ProjectInfo, UsageReport, UsageStats } from "../api/generated";
import type { HostCompatibility } from "../api/remoteHosts";
import { type ConnectionStatus, type Transport, toCommandError } from "../api/transport";
import { changesActivity, mergeActivity, rememberRunning } from "./activity";
import { Attachments, stagedIds } from "./attachments";
import { upsertChat } from "./chats";
import { type Drafts, memoryDrafts } from "./drafts";
import { changesGit, endsOrchestratorTurn, EVERYTHING, isAppWide, withTimeout } from "./eventRules";
import { frameScheduler, LiveBatch, type LiveScheduler } from "./liveBatch";
import { memoryPrefs, type Prefs } from "./prefs";
import {
  applyDurable,
  applyLive,
  applySnapshot,
  clearStreaming,
  liveOf,
  type LiveView,
  markResuming,
  modelOf,
  newProjectView,
  NO_LIVE,
  prependHistory,
  type ProjectModel,
  type ProjectView,
  removeChatNow,
  viewOf,
} from "./project";
import { SettingsCommands } from "./settingsCommands";

export type { RemoteEventBody, SettingsChange } from "./settingsCommands";

type Expect<T extends ApiResponse["type"]> = Extract<ApiResponse, { type: T }>;

export const EVENT_PAGE = 500;
/** Events read for the transcripts when a project loads (older ones on demand). */
export const HISTORY_WINDOW = 400;
/** Events per "load older history" page. */
export const HISTORY_PAGE = 400;
/** Commits of the git panel. */
export const GIT_COMMITS = 120;
/** Delay before re-reading git after a domain change (changes come in bursts). */
export const GIT_REFRESH_MS = 400;
/** How long the branch PR / merge statuses are reused before `gh` is asked again. */
export const BRANCH_STATUS_TTL_MS = 60_000;
/** Delay before the project list is re-read after work started or ended in a project not on screen. */
export const ACTIVITY_REFRESH_MS = 200;
/** Preference key: path of the project opened last (reopened after a reload). */
export const LAST_PROJECT_KEY = "yhtye.lastProject";
/** Preference key (per project id): the chat shown when the project is opened again. */
export const chatPrefKey = (project: string, scope = ""): string => `${scope}yhtye.chat.${project}`;
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
  /**
   * Chats made since git was last read (`chat_created`, maybe by another client
   * in a new worktree): not shown as "not found" until git is read again.
   */
  awaiting: string[];
  /** PR / merge status per branch (the dots of the BRANCHES tree). */
  statuses: BranchStatuses | null;
  /** When `statuses` was last read (ms since the epoch). */
  statusesAt: number;
}

/** The order of the open project's branches (the core's list, most recently used first). */
export interface BranchOrderView {
  project: string;
  /** Worktree paths and branch names; see `branchTree.ts`. */
  keys: string[];
}

/** Subscription usage for the status bar (read from the harness by the core). */
export interface UsageView {
  report: UsageReport | null;
  /** Why the last read failed (the meters then stay empty). */
  error: string | null;
  loading: boolean;
}

/** Token usage statistics of the dashboard (read from the agents' own logs by the core). */
export interface StatsView {
  report: UsageStats | null;
  /** Why the last read failed. */
  error: string | null;
  loading: boolean;
}

/** The machine the core runs on, as its last answer to a ping told. */
export interface HostView {
  /** Its host name (`null` until the first answer). */
  name: string | null;
  /** Its Yhtye version (`null` until the first answer, or if it predates the field). */
  version: string | null;
  /**
   * Whether that version works with this app, for a remote host (the shell
   * judges it when it connects). `null` for the local core, and while unknown.
   * The UI warns when it is `incompatible`.
   */
  compatibility: HostCompatibility | null;
  /** It runs without a GUI (`yhtye --headless`); a core that does not say is taken as having one. */
  headless: boolean;
  /** When the core last answered a ping (ms since the epoch). */
  lastPingAt: number | null;
}

export interface AppStoreOptions {
  /** Where the last project is remembered (default: in memory only). */
  prefs?: Prefs;
  /** Unsent composer text (default: this store's own, in memory). */
  drafts?: Drafts;
  /**
   * Prepended to the keys this store saves under and the UI builds from project
   * ids (`scopedKey`): "" for the local core (the keys stay what they were),
   * "host:<server id>:" for a remote host.
   */
  keyScope?: string;
  /**
   * Open the project that was open last when the connection first comes up
   * (default: yes). Off for a remote host: connecting to it must not start
   * anything there until the person picks a project.
   */
  reopenLast?: boolean;
  historyWindow?: number;
  historyPage?: number;
  /**
   * When the live events collected so far (streamed text, usage) are shown:
   * the next animation frame, or after `LIVE_FLUSH_MAX_MS` (default). Tests
   * pass their own, or call `flushLive`.
   */
  liveScheduler?: LiveScheduler;
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
  /**
   * The open project's live part (`LiveView`), apart from `project`: it changes
   * with every chunk, so a component reads only the session it shows.
   */
  live: LiveView;
  errors: AppError[];
  busy: { opening: boolean; sending: boolean; compacting: boolean };
  git: GitView | null;
  /** The branch order of the open project (`null` until it was read). */
  branchOrder: BranchOrderView | null;
  usage: UsageView;
  /** The usage statistics of the dashboard (read on demand, see `refreshStats`). */
  stats: StatsView;
  host: HostView;
  /** The paired devices (the core's `list_clients`), as the core ordered them. */
  clients: ClientDevice[];
}

export class AppStore extends SettingsCommands {
  private state: AppState;
  private readonly listeners = new Set<() => void>();
  private readonly unsubscribe: (() => void)[] = [];
  private buffer: ApiEvent[] = [];
  /** Live events of the open project, shown once per frame (see `liveBatch.ts`). */
  private readonly liveBatch: LiveBatch;
  private syncing = false;
  private everOpen = false;
  private loadToken = 0;
  private nextErrorId = 1;
  private gitTimer: ReturnType<typeof setTimeout> | null = null;
  private gitToken = 0;
  private activityTimer: ReturnType<typeof setTimeout> | null = null;
  private activityToken = 0;
  private usageTimer: ReturnType<typeof setInterval> | null = null;
  /** Bumped by every `refreshStats`: only the newest answer is kept (the period may change meanwhile). */
  private statsToken = 0;
  /** This store's host is the one on screen (the meters are read only then, see `setForeground`). */
  private foreground = true;
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private pinging = false;
  /** Bumped by every read and `clients_changed`: only the newest one is kept. */
  private clientsToken = 0;
  /** Bumped by every read, drag and `branch_order_changed`: only the newest answer is kept. */
  private branchOrderToken = 0;
  /** Per-device preferences; the UI keeps its own small ones (panel sizes) here too. */
  readonly prefs: Prefs;
  /** Unsent composer text per chat (kept while another chat or project is shown). */
  readonly drafts: Drafts;
  /** The draft's attachments (staged as they are added) and the previews of sent ones. */
  readonly attachments: Attachments;
  /** See {@link AppStoreOptions.keyScope}. */
  readonly keyScope: string;
  private readonly reopenOnConnect: boolean;
  private readonly historyWindow: number;
  private readonly historyPage: number;

  constructor(transport: Transport, options: AppStoreOptions = {}) {
    super(transport);
    this.prefs = options.prefs ?? memoryPrefs();
    this.drafts = options.drafts ?? memoryDrafts();
    this.keyScope = options.keyScope ?? "";
    this.reopenOnConnect = options.reopenLast ?? true;
    this.historyWindow = options.historyWindow ?? HISTORY_WINDOW;
    this.historyPage = options.historyPage ?? HISTORY_PAGE;
    this.liveBatch = new LiveBatch(options.liveScheduler ?? frameScheduler, (events) =>
      this.commit((v) => events.reduce(applyLive, v)),
    );
    this.attachments = new Attachments(transport, this.drafts, (message) => this.pushError(message));
    this.state = {
      connection: { state: "connecting" },
      transport: { kind: transport.kind, target: transport.target },
      projects: [],
      project: null,
      live: NO_LIVE,
      errors: [],
      busy: { opening: false, sending: false, compacting: false },
      git: null,
      branchOrder: null,
      usage: { report: null, error: null, loading: false },
      stats: { report: null, error: null, loading: false },
      host: { name: null, version: null, compatibility: null, headless: false, lastPingAt: null },
      clients: [],
    };
  }

  start(): void {
    this.unsubscribe.push(this.transport.subscribe((ev) => this.onEvent(ev)));
    this.unsubscribe.push(this.transport.onStatus((s) => this.onStatus(s)));
  }

  stop(): void {
    for (const un of this.unsubscribe.splice(0)) un();
    this.liveBatch.clear();
    if (this.gitTimer) clearTimeout(this.gitTimer);
    this.gitTimer = null;
    if (this.activityTimer) clearTimeout(this.activityTimer);
    this.activityTimer = null;
    this.stopUsagePolling();
    this.stopPinging();
  }

  getState = (): AppState => this.state;

  /**
   * Shows the live events that arrived since the last frame now, instead of at
   * the next one (the stored events do so by themselves: they come after what
   * is waiting). For tests and for code that reads the streamed text.
   */
  flushLive(): void {
    this.liveBatch.flush();
  }

  /** A key for something of project `id` (a React key, ...) that is not the same one on another host. */
  scopedKey(id: string): string {
    return this.keyScope + id;
  }

  /**
   * Whether this store's host is the one on screen. The subscription meters are
   * read only for the host on screen: a host that is only connected to has no
   * use for them, and reading them makes it query its harness. Stores are in the
   * foreground until told otherwise.
   */
  setForeground(foreground: boolean): void {
    if (foreground === this.foreground) return;
    this.foreground = foreground;
    if (!foreground) this.stopUsagePolling();
    else if (this.state.connection.state === "open") this.startUsagePolling();
  }

  /** The shell's verdict on the host's version (a remote host's; see {@link HostView.compatibility}). */
  noteHostCompatibility(compatibility: HostCompatibility | null): void {
    const host = this.state.host;
    if (JSON.stringify(host.compatibility) === JSON.stringify(compatibility)) return;
    this.set({ host: { ...host, compatibility } });
  }

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
    try {
      await this.tryOpenProject(path);
    } catch (e) {
      this.pushError(`open_project failed: ${toCommandError(e).message}`);
    }
  }

  /**
   * `openProject` for a form that shows the refusal next to its input (the
   * dashboard's "add a project"): rejects with the core's error instead of
   * putting it in the banners.
   */
  async tryOpenProject(path: string): Promise<void> {
    this.set({ busy: { ...this.state.busy, opening: true } });
    try {
      const r = await this.invoke({ type: "open_project", path }, "project");
      // Opening a project uses it: it is first now (the core says so too).
      this.set({ projects: toFrontProject(this.state.projects, r.project) });
      await this.load(r.project);
    } finally {
      this.set({ busy: { ...this.state.busy, opening: false } });
    }
  }

  /**
   * Puts the projects in the order the user made (`ids` is the whole list).
   * Shown at once; the core's listing replaces it, and a refusal puts the
   * order back as the core has it.
   */
  async reorderProjects(ids: string[]): Promise<void> {
    const before = this.state.projects;
    this.set({ projects: ids.flatMap((id) => before.filter((p) => p.id === id)) });
    const r = await this.run({ type: "reorder_projects", ids }, "projects");
    if (r) this.set({ projects: r.projects });
    else void this.refreshProjects();
  }

  /**
   * Puts the open project's branches in the order the user made: `keys` is the
   * order of the items on screen (worktree paths and branch names); the core
   * keeps the others where they were. Shown at once.
   */
  async reorderBranches(keys: string[]): Promise<void> {
    const project = this.state.project?.info.id;
    if (!project) return;
    const token = ++this.branchOrderToken;
    const known = this.state.branchOrder?.project === project ? this.state.branchOrder.keys : [];
    this.set({ branchOrder: { project, keys: mergeKeys(keys, known) } });
    const r = await this.run({ type: "reorder_branches", project, keys }, "branch_order");
    if (!r) return void this.refreshBranchOrder();
    if (token === this.branchOrderToken) this.setBranchOrder(project, r.keys);
  }

  /**
   * Sends `text` with the chat's staged draft attachments; returns whether it
   * was accepted (the composer keeps the text and the attachments otherwise).
   */
  async sendMessage(text: string): Promise<boolean> {
    const view = this.state.project;
    if (!view?.selectedChat) return false;
    const chat = view.selectedChat;
    const project = view.info.id;
    const files = this.drafts.attachments(project, chat).filter((a) => a.status === "staged");
    const ids = stagedIds(files);
    this.set({ busy: { ...this.state.busy, sending: true } });
    try {
      const cmd: ApiCommand = { type: "send_user_message", project, chat, text, ...(ids.length > 0 ? { attachments: ids } : {}) };
      const r = await this.run(cmd, "accepted");
      if (r !== null) this.attachments.sent(project, chat, files);
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

  /**
   * Has the selected chat's idle orchestrator compact its context (`/compact`,
   * sent as is). Refusals (not running, in a turn) are shown as errors.
   */
  async compactChat(): Promise<void> {
    const view = this.state.project;
    if (!view?.selectedChat || this.state.busy.compacting) return;
    this.set({ busy: { ...this.state.busy, compacting: true } });
    try {
      await this.run({ type: "compact_chat", project: view.info.id, chat: view.selectedChat }, "accepted");
    } finally {
      this.set({ busy: { ...this.state.busy, compacting: false } });
    }
  }

  /**
   * Shows another chat; remembered for the project. Opening a chat brings its
   * orchestrator back if it has something to resume (a restart of Yhtye starts
   * none), which `openChat` asks the core for.
   */
  selectChat(chat: string): void {
    const view = this.state.project;
    if (!view || view.selectedChat === chat || !view.chats.some((c) => c.id === chat)) return;
    this.prefs.set(chatPrefKey(view.info.id, this.keyScope), chat);
    const { [chat]: _read, ...unread } = view.unread;
    this.updateProject((v) => ({ ...v, selectedChat: chat, unread }));
    void this.openChat(view.info.id, chat);
  }

  /**
   * Tells the core the user is looking at `chat`: it starts the chat's
   * orchestrator if there is a stored session or work to resume, and answers
   * whether it is starting (shown until its session is up). Failures are shown.
   */
  private async openChat(project: string, chat: string): Promise<void> {
    const r = await this.run({ type: "open_chat", project, chat }, "chat_opened");
    if (this.state.project?.info.id !== project) return;
    this.updateProject((v) => markResuming(v, chat, r?.starting ?? false));
  }

  /** `openChat` for the chat on screen (after a project is loaded or the connection came back). */
  private openSelectedChat(): void {
    const view = this.state.project;
    if (view?.selectedChat) void this.openChat(view.info.id, view.selectedChat);
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
    if ("branch" in target) await this.readGitFor(r.chat);
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
    await this.readGitFor(r.chat);
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
    this.drafts.clear(project, chat);
    if (this.state.project?.info.id !== project) return;
    this.updateChats((v) => removeChatNow(v, chat));
  }

  /**
   * Puts a chat to sleep: stops its orchestrator process (its stored session
   * is kept, so the next message brings it back). Refusals (a turn is running)
   * are shown as errors.
   */
  async sleepChat(chat: string): Promise<void> {
    const project = this.state.project?.info.id;
    if (!project) return;
    await this.run({ type: "sleep_chat", project, chat }, "accepted");
  }

  /**
   * Puts every chat of a project to sleep (any project, not only the one on
   * screen): stops all their orchestrator processes, so the project is no
   * longer awake. All or nothing: a refusal (a turn is running in one of the
   * chats) is shown as an error. The project list follows the `session_stopped`
   * events of a project that is not on screen.
   */
  async sleepProject(project: string): Promise<void> {
    await this.run({ type: "sleep_project", project }, "accepted");
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

  /**
   * Loads the page of history before what is shown (lazy history, newest first).
   * A failure is kept in `olderError` for the history row to show; calling this
   * again retries.
   */
  async loadOlderHistory(): Promise<void> {
    const view = this.state.project;
    if (!view || view.phase !== "ready" || view.loadingOlder || view.historyStart <= 1) return;
    const token = this.loadToken;
    const from = Math.max(1, view.historyStart - this.historyPage);
    this.updateProject((v) => ({ ...v, loadingOlder: true, olderError: null }));
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
      this.updateProject((v) => ({ ...v, loadingOlder: false, olderError: toCommandError(e).message }));
    }
  }

  /** Reads the open project's branch order again (a failure keeps the last one). */
  async refreshBranchOrder(): Promise<void> {
    const project = this.state.project?.info.id;
    if (!project) return;
    const token = ++this.branchOrderToken;
    try {
      const r = await this.invoke({ type: "get_branch_order", project }, "branch_order");
      if (token === this.branchOrderToken) this.setBranchOrder(project, r.keys);
    } catch {
      // Kept as it was: an order is only a preference.
    }
  }

  private setBranchOrder(project: string, keys: string[]): void {
    if (this.state.project?.info.id !== project) return;
    this.set({ branchOrder: { project, keys } });
  }

  /** Re-reads the open project's branches and commit graph. */
  async refreshGit(): Promise<void> {
    const view = this.state.project;
    if (!view) return;
    const project = view.info.id;
    const token = ++this.gitToken;
    const prev = this.state.git?.project === project ? this.state.git : null;
    const awaiting = prev?.awaiting ?? [];
    const statuses = prev?.statuses ?? null;
    const statusesAt = prev?.statusesAt ?? 0;
    this.set({ git: { project, overview: prev?.overview ?? null, error: null, loading: true, awaiting, statuses, statusesAt } });
    try {
      const r = await this.invoke({ type: "get_git_overview", project, limit: GIT_COMMITS }, "git_overview");
      if (token !== this.gitToken) return;
      // Read after every chat it was waiting for was made: their worktrees are in it.
      this.set({ git: { project, overview: r.git, error: null, loading: false, awaiting: [], statuses, statusesAt } });
      void this.refreshBranchStatuses();
    } catch (e) {
      if (token !== this.gitToken) return;
      const error = toCommandError(e).message;
      this.set({ git: { ...(this.state.git ?? { project, overview: null, awaiting, statuses, statusesAt }), project, error, loading: false } });
    }
  }

  /**
   * Re-reads the branches' PR / merge statuses (the dots of the BRANCHES
   * tree), unless they were read recently: `gh` uses the network. A failure
   * keeps the last ones (the dots then read "unknown" until the next read).
   */
  async refreshBranchStatuses(force = false): Promise<void> {
    const view = this.state.project;
    if (!view) return;
    const project = view.info.id;
    const prev = this.state.git?.project === project ? this.state.git : null;
    if (!force && prev?.statuses && Date.now() - prev.statusesAt < BRANCH_STATUS_TTL_MS) return;
    try {
      const r = await this.invoke({ type: "get_branch_statuses", project }, "branch_statuses");
      if (this.state.project?.info.id !== project) return;
      const git = this.state.git?.project === project
        ? this.state.git
        : { project, overview: null, error: null, loading: false, awaiting: [] as string[], statuses: null as BranchStatuses | null, statusesAt: 0 };
      this.set({ git: { ...git, project, statuses: r.statuses, statusesAt: Date.now() } });
    } catch {
      // Kept as they were (see above).
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

  /**
   * Reads the token usage statistics of the last `days` days (the dashboard),
   * cut into days in this client's time zone. `refresh` asks the core to scan
   * the logs again instead of answering from its cache. The newest call wins:
   * choosing another period while one is read drops the older answer. A
   * failure keeps no report: the dashboard shows the reason.
   */
  async refreshStats(days: number, refresh = false): Promise<void> {
    const token = ++this.statsToken;
    this.set({ stats: { ...this.state.stats, loading: true } });
    try {
      const r = await this.invoke({ type: "get_usage_stats", days, tz_offset_min: clientTzOffsetMin(), refresh }, "usage_stats");
      if (token === this.statsToken) this.set({ stats: { report: r.stats, error: null, loading: false } });
    } catch (e) {
      if (token === this.statsToken) this.set({ stats: { report: null, error: toCommandError(e).message, loading: false } });
    }
  }

  /**
   * The folders of `path` on the machine this store's core runs on (its home
   * directory when omitted): what the folder browser of "add a project" lists
   * where the OS dialog cannot reach the host. Rejects with the core's error
   * (not found, not a directory, not allowed) for the browser to show.
   */
  async listDirectory(path?: string): Promise<DirectoryListing> {
    const r = await this.invoke({ type: "list_directory", ...(path ? { path } : {}) }, "directory");
    return r.directory;
  }

  /** Shows a message in the error banners (for what the host management layer could not do). */
  showError(message: string): void {
    this.pushError(message);
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
    // The project left is judged by its flag from now on: keep what its view said.
    const left = this.state.project?.info.id === info.id ? null : this.state.project;
    this.liveBatch.clear(); // for the view that is replaced
    this.set({ ...splitModel(newProjectView(info), null, NO_LIVE), projects: rememberRunning(this.state.projects, left), git: keepGit });
    if (left) this.scheduleActivityRefresh();
    this.prefs.set(this.keyScope + LAST_PROJECT_KEY, info.path);
    void this.refreshGit();
    void this.refreshBranchOrder();
    try {
      const snap = await this.invoke({ type: "get_snapshot", project: info.id }, "snapshot");
      if (token !== this.loadToken) return;
      const remembered = this.prefs.get(chatPrefKey(info.id, this.keyScope));
      this.updateProject((v) => applySnapshot(v, snap.snapshot, this.historyWindow, remembered));
      await this.catchUp(token);
      if (token !== this.loadToken) return;
      this.updateProject((v) => ({ ...v, phase: "ready" }));
      this.openSelectedChat();
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
        this.updateChats((v) =>
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
    if (isAppWide(ev)) {
      this.onAppWide(ev.body);
      return;
    }
    const view = this.state.project;
    if (!view || ev.project !== view.info.id) {
      // Not on screen: nothing to show of it but whether work is on (its ring).
      if (changesActivity(ev)) this.scheduleActivityRefresh();
      return;
    }
    if (view.phase === "error") return;
    if (!ev.live && ev.body.type === "domain" && ev.body.event.type === "chat_created") {
      // Its worktree may be new: not "not found" while git is read again, at once.
      void this.readGitFor(ev.body.event.chat);
    } else if ((!ev.live && changesGit(ev)) || endsOrchestratorTurn(ev)) {
      this.scheduleGitRefresh();
    }
    if (this.syncing) {
      this.buffer.push(ev);
      return;
    }
    if (ev.live) {
      // Older than what is applied: its block may already be complete (replayed
      // from the buffer after a catch-up); showing it again would duplicate text.
      if (ev.seq < view.cursor) return;
      if (ev.seq > view.cursor) this.requestCatchUp();
      this.liveBatch.push(ev);
      return;
    }
    if (ev.seq <= view.cursor) return; // already applied (from the log or a replay)
    if (ev.seq === view.cursor + 1) {
      this.updateChats((v) => applyDurable(v, ev));
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
    if (this.foreground) this.startUsagePolling();
    this.startPinging();
    void this.refreshClients();
    const reconnect = this.everOpen;
    this.everOpen = true;
    const view = this.state.project;
    if (reconnect) this.settingsChanges.emit(EVERYTHING); // missed while disconnected
    if (reconnect && view) {
      void this.refreshProjects();
      void this.resync(view.info);
      void this.refreshGit();
      void this.refreshBranchOrder();
    } else if (this.reopenOnConnect) {
      void this.reopenLast();
    } else {
      void this.refreshProjects();
    }
  }

  /** An event of the whole app (no project): from another client, or a resync hint. */
  private onAppWide(body: ApiEventBody): void {
    switch (body.type) {
      case "projects_changed":
        void this.refreshProjects();
        return;
      case "branch_order_changed":
        this.branchOrderToken++; // newer than a read on its way
        this.setBranchOrder(body.project, body.keys);
        return;
      case "settings_changed":
        this.settingsChanges.emit({ kind: body.kind, project: body.project ?? null, revision: body.revision ?? null });
        return;
      case "resync":
        this.catchUpAll();
        return;
      case "clients_changed":
        this.clientsToken++; // newer than a read on its way
        this.set({ clients: body.clients.devices });
        return;
      default:
        return; // remote access: `onRemoteEvent`
    }
  }

  /**
   * Events were dropped for this client (it fell behind): read again what they
   * would have changed — the project list, the open project's log and git, the
   * chat on screen (whether it is starting), and the settings on display.
   */
  private catchUpAll(): void {
    void this.refreshProjects();
    void this.refreshClients();
    this.settingsChanges.emit(EVERYTHING);
    const view = this.state.project;
    if (view?.phase !== "ready") return;
    this.requestCatchUp();
    void this.refreshGit();
    void this.refreshBranchOrder();
    this.openSelectedChat();
  }

  /** After a (re)load of the page: open the project that was open last. */
  private async reopenLast(): Promise<void> {
    await this.refreshProjects();
    const last = this.prefs.get(this.keyScope + LAST_PROJECT_KEY);
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
      this.set({
        host: {
          ...this.state.host,
          name: r.host,
          version: r.version ? r.version : null,
          headless: r.headless === true,
          lastPingAt: Date.now(),
        },
      });
    } catch {
      // Missed; the next ping tries again.
    } finally {
      this.pinging = false;
    }
  }

  /**
   * Reads the paired devices. A failure is not shown: the footer keeps the
   * last list, and the next reconnect or resync reads again.
   */
  private async refreshClients(): Promise<void> {
    const token = ++this.clientsToken;
    try {
      const r = await this.invoke({ type: "list_clients" }, "clients");
      if (token === this.clientsToken) this.set({ clients: r.clients.devices });
    } catch {
      // Kept as it was (see above).
    }
  }

  /**
   * Re-reads which projects have work on, soon: the events of a project that is
   * not on screen are not folded, so the core is asked (changes come in bursts).
   */
  private scheduleActivityRefresh(): void {
    if (this.activityTimer) return;
    this.activityTimer = setTimeout(() => {
      this.activityTimer = null;
      void this.refreshActivity();
    }, ACTIVITY_REFRESH_MS);
  }

  /** Takes `open` / `running` of every project from the core (the last answer wins). */
  private async refreshActivity(): Promise<void> {
    const token = ++this.activityToken;
    const r = await this.run({ type: "list_projects" }, "projects");
    if (!r || token !== this.activityToken) return;
    const projects = mergeActivity(this.state.projects, r.projects);
    if (projects !== this.state.projects) this.set({ projects });
  }

  /** `chat` was made after git was last read (see `GitView.awaiting`). */
  private awaitGitFor(chat: string): void {
    const git = this.state.git;
    const project = this.state.project?.info.id;
    if (!project || git?.project !== project || git.awaiting.includes(chat)) return;
    this.set({ git: { ...git, awaiting: [...git.awaiting, chat] } });
  }

  /**
   * Reads git at once because `chat` was made (a pending delayed read is
   * dropped), unless it needs no read: its worktree is in the overview already,
   * or a read waiting for it is in flight. That is the case for this client's
   * own `create_chat` / `create_branch` (it reads git before adopting the chat)
   * and for an event seen again (replayed from the buffer after a catch-up).
   */
  private readGitFor(chat: Pick<ChatInfo, "id" | "worktree">): Promise<void> {
    if (this.gitCovers(chat)) return Promise.resolve();
    this.awaitGitFor(chat.id);
    if (this.gitTimer) clearTimeout(this.gitTimer);
    this.gitTimer = null;
    return this.refreshGit();
  }

  /** Git has been read since `chat` was made, or is being read for it. */
  private gitCovers(chat: Pick<ChatInfo, "id" | "worktree">): boolean {
    const git = this.state.git;
    if (!git || git.project !== this.state.project?.info.id) return false;
    if (git.overview?.worktrees.some((w) => w.path === chat.worktree)) return true;
    return git.loading && git.awaiting.includes(chat.id);
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
    this.openSelectedChat(); // the core may have restarted
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


  private pushError(message: string): void {
    const error = { id: this.nextErrorId++, message };
    this.set({ errors: [...this.state.errors, error].slice(-MAX_ERRORS) });
  }

  /**
   * `updateProject` for changes that may add or remove chats (possibly another
   * client's): the drafts of deleted chats go, and when the chat on screen
   * changed (it was deleted, or the project had none and got one), the one
   * shown now is remembered and opened like a chat the user picked.
   */
  private updateChats(f: (v: ProjectModel) => ProjectModel): void {
    const before = this.state.project;
    this.updateProject(f);
    const after = this.state.project;
    if (!before || !after || before.info.id !== after.info.id || before.chats === after.chats) return;
    const project = after.info.id;
    for (const c of before.chats) {
      if (!after.chats.some((x) => x.id === c.id)) this.drafts.clear(project, c.id);
    }
    const next = after.selectedChat;
    if (next === before.selectedChat) return;
    this.prefs.set(chatPrefKey(project, this.keyScope), next ?? "");
    if (next && after.phase === "ready") void this.openChat(project, next);
  }

  /**
   * Changes the open project's view. The live events that are waiting come
   * first, so that they never swap places with what `f` does.
   */
  private updateProject(f: (v: ProjectModel) => ProjectModel): void {
    this.liveBatch.flush();
    this.commit(f);
  }

  /** `updateProject` without the flush (the flush itself, which `f` is part of). */
  private commit(f: (v: ProjectModel) => ProjectModel): void {
    const view = this.state.project;
    if (!view) return;
    const model = modelOf(view, this.state.live);
    const next = f(model);
    if (next === model) return;
    const patch = splitModel(next, view, this.state.live);
    if (patch.project !== view || patch.live !== this.state.live) this.set(patch);
  }

  private set(patch: Partial<AppState>): void {
    this.state = { ...this.state, ...patch };
    this.notify();
  }

  private notify(): void {
    for (const l of [...this.listeners]) l();
  }
}

/** `model` as the two parts of the state; a part that did not change is the object it was. */
function splitModel(model: ProjectModel, view: ProjectView | null, live: LiveView): { project: ProjectView; live: LiveView } {
  return { project: viewOf(model, view), live: liveOf(model, live) };
}

/** This client's UTC offset in minutes, east positive (what `get_usage_stats` cuts the days with). */
export function clientTzOffsetMin(): number {
  // `getTimezoneOffset` is how far UTC is behind local time (negative east of Greenwich); `|| 0` drops a -0.
  return -new Date().getTimezoneOffset() || 0;
}

/** `p` first, replacing its entry (or added when the list did not have it). */
function toFrontProject(list: ProjectInfo[], p: ProjectInfo): ProjectInfo[] {
  return [p, ...list.filter((x) => x.id !== p.id)];
}

/** `keys`, then the ones of `rest` they do not name (what the core stores for a reorder). */
function mergeKeys(keys: string[], rest: string[]): string[] {
  return [...keys, ...rest.filter((k) => !keys.includes(k))];
}
