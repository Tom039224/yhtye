// Test-only transport and a small in-memory core that serves a recorded or
// constructed event log. Never used on app runtime paths.

import type {
  AgentChoice,
  AgentRole,
  AgentSettingsLayer,
  AgentSettingsView,
  ApiCommand,
  ApiEvent,
  ApiResponse,
  GitOverview,
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
  /** Commands that should fail, by type. */
  failures = new Map<ApiCommand["type"], CommandError>();
  /** Largest page the fake returns (whatever the client asks for). */
  pageCap = Number.MAX_SAFE_INTEGER;
  /** Served for `get_git_overview`. */
  git: GitOverview = { head: "main", head_sha: null, branches: [], commits: [], truncated: false };
  /** Served for `get_usage` (`null`: the core reports it unavailable). */
  usage: UsageReport | null = null;
  /** Agent settings served by `get/set_agent_settings` (a small model of the core's layers). */
  agents = new FakeAgentSettings();
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
      case "get_git_overview":
        return { type: "git_overview", git: this.git };
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
  harnesses: HarnessInfo[] = [{ id: "claude-code", label: "Claude Code", requires_model: false, orchestrator_read_only: true }];
  builtin: AgentChoice = { harness: "claude-code", model: "haiku" };
  global: AgentSettingsLayer = emptyLayer();
  projects = new Map<string, AgentSettingsLayer>();
  /** Served by `list_harness_models`; a missing harness fails (unavailable). */
  models: Record<string, HarnessModels> = {
    "claude-code": {
      harness: "claude-code",
      models: [
        { value: "default", name: "Default (recommended)", description: null },
        { value: "haiku", name: "Haiku", description: null },
        { value: "sonnet", name: "Sonnet", description: null },
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
      projectLayer?.[r] ?? this.global[r] ?? { candidates: [this.builtin], default: this.builtin };
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
