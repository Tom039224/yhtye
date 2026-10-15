// Entry of `layout-harness.html` (dev only, never in the build): the real App on an in-memory
// core, in a state picked by `?scenario=` (see SCENARIOS), so that scripts/check-layout.mjs can
// look at the page in a real browser. Not a test; vitest does not load it.

import "../styles/tokens.css";

import React from "react";
import ReactDOM from "react-dom/client";

import type { ApiEvent, BranchState, BranchStatuses, ChatInfo, GitCommit, GitOverview, Group, HarnessModels, Help, RemoteDevice, SessionRecord, Snapshot, State, Task } from "../api/generated";
import App from "../App";
import { AppStore } from "../store/app";
import { orchestratorKey } from "../store/chats";
import { emptyState } from "../store/domain";
import { CommandError } from "../api/transport";
import { memoryPrefs } from "../store/prefs";
import { LOCAL_HOST } from "../store/hosts";
import { HostsProvider } from "../store/useHosts";
import { StoreContext } from "../store/useStore";
import { agentText, delivered, ev, turnEnded, userMessage } from "./events";
import { durable, FULL_RUN, PROJECT } from "./fixtures";
import { FakeCore, MemoryTransport } from "./memoryTransport";
import { FakeHostsApi, hostInfo, newTestHosts, type TestHosts } from "./remoteHosts";

/**
 * What a scenario opens: nothing, the recorded run, the recorded run with a few paired devices
 * under the host (one connected, one offline with a long name), a project with far more
 * content than fits (12 devices among it: folded into "他 N 台"), the same with group and task
 * names of the most characters there can be (32) instead of the older `G-1` / `T-1`, or the
 * recorded run with other Yhtye hosts paired with (one connected with a long name, one revoked,
 * one waiting to reconnect).
 */
const SCENARIOS = ["empty", "run", "devices", "long", "names", "hosts"] as const;
type Scenario = (typeof SCENARIOS)[number];

declare global {
  interface Window {
    /** For the checking script: drive the in-memory core and the store from the page. */
    __yhtye?: {
      store: AppStore;
      core: FakeCore;
      transport: MemoryTransport;
      failCommand: (type: string, message: string) => void;
      /** The hosts scenario: the registry and the shell's fake (to push a pairing link, ...). */
      hosts?: TestHosts;
    };
  }
}

const NOW = Date.now();
const NO_SPACES = "super-long-name-without-any-break-opportunity-".repeat(6);
const PARAGRAPH =
  "これは長い返答の段落です。ウィンドウの高さを何倍も超える会話でも、ページ全体ではなく会話パネルの中だけがスクロールします。".repeat(3);
const CHAT_ID = "C-1";
const EXCHANGES = 160;
const TASKS = 14;
const WORKTREES = 12;
const COMMITS = 80;

/** The ids of the long scenarios: the older numbered ones, or 32-character names (orchestration-model §2.1). */
interface Ids {
  group: string;
  task: (i: number) => string;
  /** A task's branch (the core's `domain::task_branch`). */
  taskBranch: (i: number) => string;
}
const NUMBERED: Ids = { group: "G-1", task: (i) => `T-${i + 1}`, taskBranch: (i) => `yhtye/G-1-T-${i + 1}` };
const NAMED_GROUP = "the-long-group-name-for-a-layout";
const namedTask = (i: number) => `the-long-task-name-for-layout-${String(i + 1).padStart(2, "0")}`;
const NAMED: Ids = { group: NAMED_GROUP, task: namedTask, taskBranch: (i) => `yhtye/${NAMED_GROUP}--${namedTask(i)}` };

function longTitle(i: number): string {
  return i % 3 === 0 ? `${NO_SPACES}${i}` : `${i} 件目の、とても長いタイトルのタスク。折り返さず省略記号で切れるはずの文章です`;
}

function longTasks(ids: Ids): { tasks: Task[]; helps: Help[] } {
  const base = FULL_RUN.end.state.tasks[0];
  const done = base.steps.map((s) => ({ ...s, status: "done" as const }));
  const running = base.steps.map((s, i) => ({ ...s, status: i === 0 ? ("running" as const) : ("pending" as const), result: null, verdict: null }));
  const helps: Help[] = [];
  const tasks = Array.from({ length: TASKS }, (_, i): Task => {
    const id = ids.task(i);
    const kind = i % 4;
    const status = (["done", "running", "handling", "pending"] as const)[kind];
    if (status === "handling") {
      helps.push({
        id: `H-${helps.length + 1}`,
        task: id,
        step: 0,
        kind: "blocked",
        message: `${PARAGRAPH} ${NO_SPACES}`,
        source: { by: "agent", role: "implementer" },
        state: "open",
        agent_lost: false,
        reply: null,
      });
    }
    const settled = status === "done";
    return {
      ...base,
      id,
      group: ids.group,
      title: longTitle(i),
      status,
      steps: settled ? done : running,
      current: settled ? base.steps.length - 1 : 0,
    };
  });
  return { tasks, helps };
}

/** Every task has had an implementer and several review rounds: the output panel's session pick-list is long. */
function longSessions(tasks: Task[]): SessionRecord[] {
  const keys = ["implementer", "review-1", "review-3", "review-5", "review-7"];
  return tasks.flatMap((t, i) =>
    keys.map((key, k): SessionRecord => ({
      session_key: `${t.id}/${key}`,
      role: k === 0 ? "implementer" : "reviewer",
      task: t.id,
      acp_session_id: `acp-${t.id}-${key}`,
      status: "live",
      turn_running: (i + k) % 3 === 0,
      agent: null,
      cwd: null,
    })),
  );
}

const BRANCH_STATES: BranchState[] = ["unmerged", "pr_open", "merged", "merged_with_changes", "unknown"];

/** Every PR / merge state in turn over the branches, so each status icon is drawn. */
function longBranchStatuses(git: GitOverview): BranchStatuses {
  const statuses = git.branches.slice(1).map((b, i) => {
    const state = BRANCH_STATES[i % BRANCH_STATES.length];
    const pr = state === "unmerged" || state === "unknown" ? null : 100 + i;
    return { branch: b.name, state, pr_number: pr, pr_url: pr == null ? null : `https://example.test/u/repo/pull/${pr}` };
  });
  return { default_branch: "main", statuses };
}

function longGit(ids: Ids): { git: GitOverview; chats: ChatInfo[]; chatStates: State["chats"] } {
  const sha = (c: string, i: number) => `${c}${i}`.padEnd(40, c);
  const branches = Array.from({ length: WORKTREES }, (_, i) => ({ name: i === 0 ? "main" : `feature/${NO_SPACES}${i}`, sha: sha("b", i) }));
  const worktrees = branches.map((b, i) => ({
    path: i === 0 ? "/repo" : `/data/worktrees/repo/branches/${b.name.replace(/\//g, "-")}`,
    branch: b.name,
    head_sha: b.sha,
    is_main: i === 0,
    missing: false,
  }));
  const commits: GitCommit[] = Array.from({ length: COMMITS }, (_, i) => ({
    sha: sha("c", i),
    parents: i + 1 < COMMITS ? [sha("c", i + 1)] : [],
    branches: i === 0 ? ["main"] : i === 1 ? [`yhtye/${ids.group}`] : i % 10 === 0 ? [`feature/${NO_SPACES}${i}`] : i % 10 === 5 ? [ids.taskBranch(i % TASKS), ids.taskBranch((i + 7) % TASKS)] : [],
    subject: `commit ${i}: ${PARAGRAPH}`,
    ts_ms: NOW - i * 3_600_000,
  }));
  const git: GitOverview = { head: "main", head_sha: sha("c", 0), branches, worktrees, commits, truncated: true };
  const chats: ChatInfo[] = [];
  worktrees.forEach((w, wi) => {
    for (let k = 0; k < 3; k++) {
      const n = chats.length + 1;
      const id = `C-${n}`;
      chats.push({ id, worktree: w.path, title: k === 0 ? `${NO_SPACES}${n}` : `${n} 番目のチャット ${PARAGRAPH}`, created_ms: NOW - n * 60_000, last_used_ms: NOW - wi * 600_000 - k * 60_000 });
    }
  });
  // The selected chat is the one the transcript below belongs to; keep it the most recent.
  chats[0] = { ...chats[0], id: CHAT_ID, last_used_ms: NOW };
  return { git, chats, chatStates: chats.map(({ id, worktree, title }) => ({ id, worktree, title })) };
}

/** A harness with hundreds of models (OpenCode's list): the settings use the searchable pick-list for it. */
function longModels(): HarnessModels {
  const models = Array.from({ length: 300 }, (_, i) => ({
    value: `provider-${i % 12}/model-${i}`,
    name: `Provider ${i % 12} · ${NO_SPACES}${i}`,
    description: null,
    efforts: [],
  }));
  return { harness: "claude-code", models, current: models[0].value, fetched_at_ms: NOW };
}

const WIDE_TABLE = `| ${Array.from({ length: 8 }, (_, i) => `column ${i}`).join(" | ")} |\n| ${Array.from({ length: 8 }, () => "---").join(" | ")} |\n| ${Array.from({ length: 8 }, () => NO_SPACES).join(" | ")} |`;

function longLog(group: Group, ids: Ids): ApiEvent[] {
  const orchestrator = orchestratorKey(CHAT_ID);
  const log: ApiEvent[] = [];
  let seq = 1;
  const push = (make: (seq: number) => ApiEvent) => void log.push(make(seq++));
  for (let i = 1; i <= EXCHANGES; i++) {
    push((n) => userMessage(n, i, `${i} 件目の依頼です。\n${PARAGRAPH}\n${NO_SPACES}`, CHAT_ID));
    push((n) => delivered(n, i, CHAT_ID));
    push((n) => agentText(n, orchestrator, `考えています (${i})。`, "thought"));
    push((n) => ev(n, { type: "agent", session: orchestrator, event: { type: "output", data: { kind: "tool_call", update: { toolCallId: `t${i}`, title: `Read ${NO_SPACES}${i}`, kind: "read" } } } }));
    push((n) => ev(n, { type: "agent", session: orchestrator, event: { type: "output", data: { kind: "tool_call_update", update: { toolCallId: `t${i}`, status: "completed" } } } }));
    push((n) => ev(n, { type: "tool_called", record: { binding: { session: orchestrator, role: "orchestrator", project: "repo", chat: CHAT_ID }, tool: "create_task", args: { title: NO_SPACES }, result: { Ok: { detail: PARAGRAPH } } } }));
    push((n) => agentText(n, orchestrator, `${i} 件目への返答です。\n\n- ${PARAGRAPH}\n- \`${NO_SPACES}\`\n\n${WIDE_TABLE}\n\n\`\`\`\n${NO_SPACES}${NO_SPACES}\n\`\`\``));
    push((n) => turnEnded(n, orchestrator));
  }
  // At the end (in view once scrolled down): the group's card and a notice naming a task.
  push((n) => ev(n, { type: "domain", event: { type: "group_created", group } }));
  const attrs: [string, string][] = [["task", ids.task(2)], ["help", "H-1"], ["group", group.id]];
  push((n) => ev(n, { type: "domain", event: { type: "inbox_queued", entry: { id: EXCHANGES + 1, chat: CHAT_ID, item: { kind: "help_raised", attrs, body: PARAGRAPH } } } }));
  return log;
}

function build(scenario: Scenario): { core: FakeCore; open: boolean } {
  if (scenario === "long" || scenario === "names") {
    const ids = scenario === "names" ? NAMED : NUMBERED;
    const { tasks, helps } = longTasks(ids);
    const { git, chats, chatStates } = longGit(ids);
    const group: Group = {
      ...FULL_RUN.end.state.groups[0],
      id: ids.group,
      chat: CHAT_ID,
      title: `グループ ${NO_SPACES}`,
      status: "active",
      group_branch: `yhtye/${ids.group}`,
    };
    const state: State = { ...emptyState("repo", { max_review_rounds: 2 }), chats: chatStates, groups: [group], tasks, helps };
    const snapshot: Snapshot = { seq: 0, state, sessions: longSessions(tasks), chats, starting: [] };
    const core = new FakeCore(PROJECT, snapshot);
    core.log = longLog(group, ids);
    core.git = git;
    core.branchStatuses = longBranchStatuses(git);
    core.usage = {
      plan: "max",
      windows: [
        { kind: "five_hour", label: "5-hour limit", percent: 42, resets_at_ms: NOW + 3_600_000 },
        { kind: "week", label: "Weekly · all models", percent: 77, resets_at_ms: NOW + 86_400_000 },
      ],
      fetched_at_ms: NOW,
    };
    core.agents.models["claude-code"] = longModels();
    core.remote.dnsName = `${NO_SPACES}.tailnet.ts.net`;
    core.remote.set(true, undefined);
    core.remote.devices = Array.from({ length: 12 }, (_, i): RemoteDevice => ({
      id: `dev-${i}`,
      name: i % 3 === 0 ? `${NO_SPACES}${i}` : `端末 ${i}`,
      kind: i % 3 === 1 ? "desktop" : "android",
      created_ms: NOW - i * 86_400_000,
      last_seen_ms: i % 2 === 0 ? NOW - i * 3_600_000 : null,
      connected: i % 4 === 0,
    }));
    core.others = Array.from({ length: 30 }, (_, i) => ({ id: `other-${i}`, name: `${NO_SPACES}${i}`, path: `/work/other-${i}`, open: false, running: false, awake: false }));
    return { core, open: true };
  }
  const core = new FakeCore(PROJECT, FULL_RUN.start);
  core.log = durable(FULL_RUN);
  if (scenario === "devices") {
    core.remote.devices = [
      { id: "dev-1", name: "Pixel 9", kind: "android", created_ms: NOW - 86_400_000, last_seen_ms: NOW, connected: true },
      { id: "dev-2", name: `Workstation ${NO_SPACES}`, kind: "desktop", created_ms: NOW, last_seen_ms: NOW - 3_600_000, connected: false },
    ];
  }
  return { core, open: scenario !== "empty" };
}

/** The recorded run with three other hosts paired with, whose projects are listed after this PC's. */
async function mountHosts(): Promise<TestHosts> {
  const long = `${NO_SPACES}host`;
  const t = newTestHosts({
    configureFar: (far) => {
      far.core.host = far.id === "sid-long" ? long : `host-${far.id}`;
      far.core.headless = far.id === "sid-long";
    },
  });
  t.localCore.log = durable(FULL_RUN);
  const api = t.api as FakeHostsApi;
  const named = (id: string, name: string) => ({ ...hostInfo(id), name });
  api.hosts = [
    named("sid-long", long),
    { ...named("sid-gone", "build-server"), status: { connection: { state: "revoked" as const }, server: null } },
    { ...named("sid-wait", "office-pc"), status: { connection: { state: "waiting_to_reconnect" as const, attempt: 2, delay_ms: 5000, reason: "refused" }, server: null } },
  ];
  window.__yhtye = { store: t.registry.activeStore, core: t.localCore, transport: t.localTransport, failCommand: () => {}, hosts: t };
  void t.registry.start();
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <HostsProvider registry={t.registry}>
        <App />
      </HostsProvider>
    </React.StrictMode>,
  );
  await t.registry.selectProject(LOCAL_HOST, PROJECT);
  return t;
}

async function main(): Promise<void> {
  const requested = new URLSearchParams(window.location.search).get("scenario") ?? "run";
  const scenario = SCENARIOS.find((s) => s === requested) ?? "run";
  if (scenario === "hosts") {
    await mountHosts();
    document.documentElement.dataset.ready = scenario;
    return;
  }
  const transport = new MemoryTransport({ state: "open" });
  const { core, open } = build(scenario);
  core.attach(transport);
  const store = new AppStore(transport, { prefs: memoryPrefs() });
  const failCommand = (type: string, message: string) =>
    core.failures.set(type as Parameters<typeof core.failures.set>[0], new CommandError("unavailable", message));
  window.__yhtye = { store, core, transport, failCommand };
  store.start();
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <StoreContext.Provider value={store}>
        <App />
      </StoreContext.Provider>
    </React.StrictMode>,
  );
  if (open) await store.openProject(PROJECT.path);
  document.documentElement.dataset.ready = scenario;
}

void main();
