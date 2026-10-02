// Test-only transport and a small in-memory core that serves a recorded or
// constructed event log. Never used on app runtime paths.

import type {
  AgentChoice,
  AgentRole,
  AgentSettingsLayer,
  AgentSettingsView,
  EffortOption,
  ApiCommand,
  ApiEvent,
  ApiResponse,
  ChatInfo,
  GitOverview,
  HarnessDetection,
  HarnessInfo,
  HarnessModels,
  RoleSettings,
  LoggedEvent,
  ProjectInfo,
  Snapshot,
  UsageReport,
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
  /** Other projects the core knows (listed after `info`; `open_project` finds them by path). */
  others: ProjectInfo[] = [];
  /** Commands that should fail, by type. */
  failures = new Map<ApiCommand["type"], CommandError>();
  /** Largest page the fake returns (whatever the client asks for). */
  pageCap = Number.MAX_SAFE_INTEGER;
  /** Served for `get_git_overview`. */
  git: GitOverview = {
    head: "main",
    head_sha: null,
    branches: [{ name: "main", sha: "m".repeat(40) }],
    worktrees: [{ path: "/repo", branch: "main", head_sha: "m".repeat(40), is_main: true, missing: false }],
    commits: [],
    truncated: false,
  };
  /** Served for `get_usage` (`null`: the core reports it unavailable). */
  usage: UsageReport | null = null;
  /** Agent settings served by `get/set_agent_settings` (a small model of the core's layers). */
  agents = new FakeAgentSettings();
  /** Harness detection served by `get_harnesses` / `detect_harnesses` / `set_harness_path`. */
  harnesses = new FakeHarnesses();
  /** Registered secret env names, and the values the core "stored" (test-only, never shown). */
  secretNames: string[] = [];
  secretValues: Record<string, string> = {};
  /** Makes the secret commands fail like an unavailable OS keyring. */
  secretError: string | null = null;
  /** Makes `create_branch` fail (an invalid or existing name). */
  branchError: CommandError | null = null;
  /** The host name `ping` answers with. */
  host = "test-host";
  /** Called before answering a command (to interleave pushed events). */
  before: ((cmd: ApiCommand) => void | Promise<void>) | null = null;

  constructor(info: ProjectInfo, snapshot: Snapshot) {
    this.info = info;
    this.snapshot = snapshot;
    // Recordings bind their chat to the temporary repository they ran in: that
    // is the main clone here, so the chat's worktree exists.
    const recorded = snapshot.chats.find((c) => c.id === "C-1")?.worktree;
    if (recorded) this.git = { ...this.git, worktrees: [{ ...this.git.worktrees[0], path: recorded }] };
  }

  attach(t: MemoryTransport): void {
    t.handler = async (cmd) => {
      await this.before?.(cmd);
      return this.handle(cmd);
    };
  }

  private newChat(worktree: string): ChatInfo {
    const n = this.snapshot.chats.length + 1;
    return { id: `C-${n}`, worktree, title: null, created_ms: 1, last_used_ms: 1 };
  }

  /** The worktree of `branch`: where it is checked out, else a new "Yhtye" one. */
  private worktreeOf(branch: string): string {
    const found = this.git.worktrees.find((w) => w.branch === branch);
    if (found) return found.path;
    const path = `/data/worktrees/repo/branches/${branch.replace(/\//g, "-")}`;
    const w = { path, branch, head_sha: "n".repeat(40), is_main: false, missing: false };
    this.git = { ...this.git, worktrees: [...this.git.worktrees, w] };
    return path;
  }

  /** Detects the harnesses and registers the installed ones, like the core's catalog. */
  private detected(): ApiResponse {
    const harnesses = this.harnesses.detect();
    this.agents.harnesses = harnesses
      .filter((h) => h.installed)
      .map((h) => this.agents.harnesses.find((known) => known.id === h.id) ?? { id: h.id, label: h.label, requires_model: h.id !== "claude-code" });
    return { type: "harnesses", harnesses };
  }

  handle(cmd: ApiCommand): ApiResponse {
    const failure = this.failures.get(cmd.type);
    if (failure) throw failure;
    switch (cmd.type) {
      case "ping":
        return { type: "pong", host: this.host };
      case "list_projects":
        return { type: "projects", projects: [this.info, ...this.others] };
      case "open_project":
        return { type: "project", project: { ...(this.others.find((p) => p.path === cmd.path) ?? this.info), open: true } };
      case "get_snapshot":
        return { type: "snapshot", snapshot: this.snapshot };
      case "list_events": {
        const limit = Math.min(cmd.limit ?? 500, this.pageCap);
        const after = this.log.filter((e) => !e.live && e.seq > cmd.after_seq);
        const events: LoggedEvent[] = after.slice(0, limit).map((e) => ({ ...e, live: false }));
        return { type: "events", events, more: after.length > limit };
      }
      case "get_git_overview":
        return { type: "git_overview", git: this.git };
      case "create_chat":
        return { type: "chat", chat: this.newChat(cmd.worktree ?? this.worktreeOf(cmd.branch ?? "")) };
      case "rename_chat": {
        const chat = this.snapshot.chats.find((c) => c.id === cmd.chat);
        if (!chat) throw new CommandError("not_found", `no chat ${cmd.chat}`);
        const title = cmd.title.trim().replace(/\s+/g, " ");
        if (!title) throw new CommandError("invalid_argument", "the title must not be empty");
        return { type: "chat", chat: { ...chat, title } };
      }
      case "delete_chat":
        if (!this.snapshot.chats.some((c) => c.id === cmd.chat)) throw new CommandError("not_found", `no chat ${cmd.chat}`);
        return { type: "accepted" };
      case "create_branch":
        if (this.branchError) throw this.branchError;
        this.git = { ...this.git, branches: [...this.git.branches, { name: cmd.name, sha: "n".repeat(40) }] };
        return { type: "chat", chat: this.newChat(this.worktreeOf(cmd.name)) };
      case "get_usage":
        if (!this.usage) throw new CommandError("unavailable", "usage: no harness is configured to report usage");
        return { type: "usage", usage: this.usage };
      case "get_agent_settings":
        return { type: "agent_settings", settings: this.agents.view(cmd.project ?? null) };
      case "set_agent_settings":
        this.agents.set(cmd.project ?? null, cmd.role, cmd.settings);
        return { type: "agent_settings", settings: this.agents.view(cmd.project ?? null) };
      case "list_harness_models": {
        const models = this.agents.models[cmd.harness];
        if (!models) throw new CommandError("unavailable", `models of ${cmd.harness}: could not start the harness`);
        return { type: "harness_models", models };
      }
      case "list_model_efforts": {
        const efforts = this.agents.efforts[`${cmd.harness}/${cmd.model}`];
        if (!efforts) throw new CommandError("unavailable", `efforts of ${cmd.harness}/${cmd.model}: could not select ${cmd.model}`);
        return { type: "model_efforts", efforts: { harness: cmd.harness, model: cmd.model, efforts } };
      }
      case "get_harnesses":
      case "detect_harnesses":
        return this.detected();
      case "set_harness_path":
        this.harnesses.setPath(cmd.harness, cmd.path);
        return this.detected();
      case "list_secret_env":
        return { type: "secret_env", names: [...this.secretNames] };
      case "set_secret_env":
        if (this.secretError) throw new CommandError("unavailable", this.secretError);
        if (!this.secretNames.includes(cmd.name)) this.secretNames = [...this.secretNames, cmd.name].sort();
        this.secretValues[cmd.name] = cmd.value;
        return { type: "secret_env", names: [...this.secretNames] };
      case "delete_secret_env":
        if (this.secretError) throw new CommandError("unavailable", this.secretError);
        this.secretNames = this.secretNames.filter((n) => n !== cmd.name);
        delete this.secretValues[cmd.name];
        return { type: "secret_env", names: [...this.secretNames] };
      default:
        return { type: "accepted" };
    }
  }
}

const ROLES: AgentRole[] = ["orchestrator", "implementer", "investigator", "reviewer"];

function emptyLayer(): AgentSettingsLayer {
  return { orchestrator: null, implementer: null, investigator: null, reviewer: null };
}

/** The core's settings layers in miniature (no validation). */
export class FakeAgentSettings {
  harnesses: HarnessInfo[] = [{ id: "claude-code", label: "Claude Code", requires_model: false }];
  builtin: AgentChoice = { harness: "claude-code", model: "haiku", effort: null };
  global: AgentSettingsLayer = emptyLayer();
  projects = new Map<string, AgentSettingsLayer>();
  /** Served by `list_model_efforts` for models whose list entry has `efforts: null`, by `harness/model`. */
  efforts: Record<string, EffortOption[]> = {};
  /** Served by `list_harness_models`; a missing harness fails (unavailable). */
  models: Record<string, HarnessModels> = {
    "claude-code": {
      harness: "claude-code",
      models: [
        { value: "default", name: "Default (recommended)", description: null, efforts: [] },
        { value: "haiku", name: "Haiku", description: null, efforts: [] },
        {
          value: "sonnet",
          name: "Sonnet",
          description: null,
          efforts: ["low", "medium", "high"].map((v) => ({ value: v, name: v, description: null })),
        },
      ],
      current: "haiku",
      fetched_at_ms: 0,
    },
  };

  set(project: string | null, role: AgentRole, settings: RoleSettings | null): void {
    const layer = project === null ? this.global : (this.projects.get(project) ?? emptyLayer());
    layer[role] = settings;
    if (project !== null) this.projects.set(project, layer);
  }

  view(project: string | null): AgentSettingsView {
    const projectLayer = project === null ? null : (this.projects.get(project) ?? emptyLayer());
    const role = (r: AgentRole): RoleSettings =>
      projectLayer?.[r] ??
      this.global[r] ?? { candidates: [{ ...this.builtin, note: "" }], default: this.builtin };
    const effective = Object.fromEntries(ROLES.map((r) => [r, role(r)])) as AgentSettingsView["effective"];
    return {
      harnesses: this.harnesses,
      builtin: this.builtin,
      global: { ...this.global },
      project,
      project_layer: projectLayer ? { ...projectLayer } : null,
      effective,
    };
  }
}

const HARNESS_SPECS = [
  { id: "claude-code", label: "Claude Code", main: "npx", also: [] },
  { id: "opencode", label: "OpenCode", main: "opencode", also: [] },
  { id: "codex", label: "Codex", main: "codex", also: ["npx"] },
  { id: "devin", label: "Devin", main: "devin", also: [] },
  { id: "minimax-code", label: "MiniMax Code", main: "mcode", also: [] },
];

/** The core's harness detection in miniature: a search result per command, and manual paths. */
export class FakeHarnesses {
  /** What the automatic search finds, by command (a missing command is not found). */
  found: Record<string, { path: string; source: "path" | "known_dir" }> = {
    npx: { path: "/usr/bin/npx", source: "path" },
  };
  /** Absolute paths of executable files (a manual path must be one). */
  executables = new Set<string>(["/usr/bin/npx"]);
  /** Stored manual paths of the main executables, by harness. */
  overrides: Record<string, string> = {};

  setPath(harness: string, path: string | null): void {
    if (!HARNESS_SPECS.some((s) => s.id === harness)) throw new CommandError("not_found", `unknown harness ${harness}`);
    if (path === null) {
      delete this.overrides[harness];
      return;
    }
    const p = path.trim();
    if (!p.startsWith("/")) throw new CommandError("invalid_argument", `${harness}: not an absolute path: ${p}`);
    if (!this.executables.has(p)) throw new CommandError("invalid_argument", `${harness}: not an executable file: ${p}`);
    this.overrides[harness] = p;
  }

  detect(): HarnessDetection[] {
    return HARNESS_SPECS.map((spec) => {
      const override = this.overrides[spec.id] ?? null;
      const overrideError = override !== null && !this.executables.has(override) ? `not an executable file: ${override}` : null;
      const auto = this.found[spec.main];
      const [main, source] =
        override !== null
          ? [overrideError ? null : override, overrideError ? ("none" as const) : ("override" as const)]
          : [auto?.path ?? null, auto?.source ?? ("none" as const)];
      const requirements = [
        { command: spec.main, found: main },
        ...spec.also.map((command) => ({ command, found: this.found[command]?.path ?? null })),
      ];
      return {
        id: spec.id,
        label: spec.label,
        installed: requirements.every((r) => r.found !== null),
        resolved_path: main,
        path_source: source,
        override_path: override,
        override_error: overrideError,
        requirements,
      };
    });
  }
}
