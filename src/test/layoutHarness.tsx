// Entry of `layout-harness.html` (dev only, never in the build): the real App on an in-memory
// core, in a state picked by `?scenario=` (see SCENARIOS), so that scripts/check-layout.mjs can
// look at the page in a real browser. Not a test; vitest does not load it.

import "../styles/tokens.css";

import React from "react";
import ReactDOM from "react-dom/client";

import type { ApiEvent, ChatInfo, GitCommit, GitOverview, HarnessModels, Help, Snapshot, State, Task } from "../api/generated";
import App from "../App";
import { AppStore } from "../store/app";
import { orchestratorKey } from "../store/chats";
import { emptyState } from "../store/domain";
import { CommandError } from "../api/transport";
import { memoryPrefs } from "../store/prefs";
import { StoreContext } from "../store/useStore";
import { agentText, delivered, ev, turnEnded, userMessage } from "./events";
import { durable, FULL_RUN, PROJECT } from "./fixtures";
import { FakeCore, MemoryTransport } from "./memoryTransport";

/** What a scenario opens: nothing, the recorded run, or a project with far more content than fits. */
const SCENARIOS = ["empty", "run", "long"] as const;
type Scenario = (typeof SCENARIOS)[number];

declare global {
  interface Window {
    /** For the checking script: drive the in-memory core and the store from the page. */
    __yhtye?: { store: AppStore; core: FakeCore; transport: MemoryTransport; failCommand: (type: string, message: string) => void };
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

function longTitle(i: number): string {
  return i % 3 === 0 ? `${NO_SPACES}${i}` : `${i} 件目の、とても長いタイトルのタスク。折り返さず省略記号で切れるはずの文章です`;
}

function longTasks(): { tasks: Task[]; helps: Help[] } {
  const base = FULL_RUN.end.state.tasks[0];
  const done = base.steps.map((s) => ({ ...s, status: "done" as const }));
  const running = base.steps.map((s, i) => ({ ...s, status: i === 0 ? ("running" as const) : ("pending" as const), result: null, verdict: null }));
  const helps: Help[] = [];
  const tasks = Array.from({ length: TASKS }, (_, i): Task => {
    const id = `T-${i + 1}`;
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
      group: "G-1",
      title: longTitle(i),
      status,
      steps: settled ? done : running,
      current: settled ? base.steps.length - 1 : 0,
    };
  });
  return { tasks, helps };
}

function longGit(): { git: GitOverview; chats: ChatInfo[]; chatStates: State["chats"] } {
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
    branches: i === 0 ? ["main"] : i % 10 === 0 ? [`feature/${NO_SPACES}${i}`] : [],
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

function longLog(): ApiEvent[] {
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
  return log;
}

function build(scenario: Scenario): { core: FakeCore; open: boolean } {
  if (scenario === "long") {
    const { tasks, helps } = longTasks();
    const { git, chats, chatStates } = longGit();
    const group = { ...FULL_RUN.end.state.groups[0], id: "G-1", chat: CHAT_ID, title: `グループ ${NO_SPACES}`, status: "active" as const };
    const state: State = { ...emptyState("repo", { max_review_rounds: 2 }), chats: chatStates, groups: [group], tasks, helps };
    const snapshot: Snapshot = { seq: 0, state, sessions: [], chats };
    const core = new FakeCore(PROJECT, snapshot);
    core.log = longLog();
    core.git = git;
    core.usage = {
      plan: "max",
      windows: [
        { kind: "five_hour", label: "5-hour limit", percent: 42, resets_at_ms: NOW + 3_600_000 },
        { kind: "week", label: "Weekly · all models", percent: 77, resets_at_ms: NOW + 86_400_000 },
      ],
      fetched_at_ms: NOW,
    };
    core.agents.models["claude-code"] = longModels();
    core.others = Array.from({ length: 30 }, (_, i) => ({ id: `other-${i}`, name: `${NO_SPACES}${i}`, path: `/work/other-${i}`, open: false }));
    return { core, open: true };
  }
  const core = new FakeCore(PROJECT, FULL_RUN.start);
  core.log = durable(FULL_RUN);
  return { core, open: scenario === "run" };
}

async function main(): Promise<void> {
  const requested = new URLSearchParams(window.location.search).get("scenario") ?? "run";
  const scenario = SCENARIOS.find((s) => s === requested) ?? "run";
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
