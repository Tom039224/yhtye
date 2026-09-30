// The Claude Design layout (Stage 6a) driven by the store: title bar branch,
// sidebar, task cards and mentions, the wait banner, the git panel, the runs
// view, lazy history and reopening the last project.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../App";
import type { GitOverview, Snapshot, State } from "../api/generated";
import { AppStore, type AppStoreOptions, LAST_PROJECT_KEY } from "../store/app";
import { emptyState } from "../store/domain";
import { memoryPrefs } from "../store/prefs";
import { ORCHESTRATOR } from "../test/events";
import { StoreContext } from "../store/useStore";
import { ev } from "../test/events";
import { durable, FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";

const LOG = durable(FULL_RUN);
const LAST = LOG[LOG.length - 1].seq;

function setup(opts: { snapshot?: Snapshot; log?: typeof LOG; git?: GitOverview; store?: AppStoreOptions } = {}) {
  const transport = new MemoryTransport();
  const core = new FakeCore(PROJECT, opts.snapshot ?? FULL_RUN.start);
  core.log = opts.log ?? LOG;
  if (opts.git) core.git = opts.git;
  core.attach(transport);
  const store = new AppStore(transport, opts.store);
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <App />
    </StoreContext.Provider>,
  );
  return { transport, core, store, user: userEvent.setup() };
}

async function open(store: AppStore) {
  await act(() => store.openProject(PROJECT.path));
  await waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
}

function snapshotOf(state: Partial<State>, seq = 0): Snapshot {
  // The recordings' chat `C-1` (on `main`) exists and is selected.
  const chat = { id: "C-1", worktree: "/repo", title: null };
  const info = { ...chat, created_ms: 1, last_used_ms: 1 };
  return { seq, state: { ...emptyState("repo", { max_review_rounds: 2 }), chats: [chat], ...state }, sessions: [], chats: [info] };
}

const base = FULL_RUN.end.state;
const doneTask = base.tasks[0];
const running: State["tasks"][number] = {
  ...doneTask,
  id: "T-2",
  title: "second task",
  status: "running",
  current: 0,
  steps: doneTask.steps.map((s, i) => ({ ...s, status: i === 0 ? "running" : "pending", result: null, verdict: null })),
};

const GIT: GitOverview = {
  head: "main",
  head_sha: "m".repeat(40),
  branches: [
    { name: "main", sha: "m".repeat(40) },
    { name: "yhtye/G-1", sha: "g".repeat(40) },
    { name: "yhtye/G-1-T-2", sha: "t".repeat(40) },
  ],
  worktrees: [{ path: "/repo", branch: "main", head_sha: "m".repeat(40), is_main: true, missing: false }],
  commits: [
    { sha: "t".repeat(40), parents: ["g".repeat(40)], branches: ["yhtye/G-1-T-2"], subject: "wip: second", ts_ms: 3 },
    { sha: "m".repeat(40), parents: ["a".repeat(40)], branches: ["main"], subject: "chore: base", ts_ms: 2 },
    { sha: "g".repeat(40), parents: ["a".repeat(40)], branches: ["yhtye/G-1"], subject: "merge T-1", ts_ms: 1 },
    { sha: "a".repeat(40), parents: [], branches: [], subject: "initial", ts_ms: 0 },
  ],
  truncated: false,
};

describe("design layout", () => {
  it("shows the checked-out branch, branches and the commit graph from git", async () => {
    const { store } = setup({ git: GIT, snapshot: snapshotOf({ groups: [{ ...base.groups[0], status: "active" }], tasks: [doneTask, running] }), log: [] });
    await open(store);
    expect(await screen.findByTestId("head-branch")).toHaveTextContent("main");
    const branches = screen.getByRole("list", { name: "branches" });
    // Yhtye's internal branches are in the graph but not in the tree.
    expect(within(branches).getByRole("button", { name: "main" })).toBeInTheDocument();
    expect(within(branches).queryByText(/yhtye\//)).not.toBeInTheDocument();
    const graph = screen.getByTestId("git-graph");
    expect(within(graph).getByText("ttttttt")).toBeInTheDocument();
    expect(within(graph).getByText("wip: second")).toBeInTheDocument();
    // Task branches are labelled with their task's live status.
    expect(within(graph).getByText("T-2 実装中")).toBeInTheDocument();
    expect(within(graph).getByText("HEAD")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "git" })).toHaveTextContent("yhtye/G-1 ← main");
  });

  it("renders task cards with progress, log, steps, footer and the group wait banner", async () => {
    const snapshot = snapshotOf({ groups: [{ ...base.groups[0], status: "active" }], tasks: [doneTask, running] }, LAST);
    const { store } = setup({ snapshot });
    await open(store);
    const card = screen.getByRole("article", { name: "task T-2" });
    expect(within(card).getByText("実装中")).toBeInTheDocument();
    expect(within(card).getByRole("progressbar")).toHaveAttribute("aria-valuenow", "17");
    const done = screen.getByRole("article", { name: "task T-1" });
    expect(within(done).getByText("同グループの他タスク完了を待機 — 送信保留")).toBeInTheDocument();
    expect(within(done).queryByRole("progressbar")).not.toBeInTheDocument();
    // The footer names the agent session that worked on it last.
    expect(within(done).getByText(/^review-1 · /)).toBeInTheDocument();
    expect(screen.getByTestId("wait-banner")).toHaveTextContent("グループ「greeting」の全タスク完了まで待機中 — 1/2 完了");
    const header = within(screen.getByRole("region", { name: "tasks" })).getAllByText(/完了$/)[0];
    expect(header).toHaveTextContent("1/2 完了");
  });

  it("quotes a task into the composer with @ and sends the reference", async () => {
    const snapshot = snapshotOf({ groups: [{ ...base.groups[0], status: "active" }], tasks: [doneTask, running] });
    const { store, user, transport } = setup({ snapshot, log: [] });
    await open(store);
    await user.click(screen.getByRole("button", { name: "T-2 をオーケストレータへ引用" }));
    await user.click(screen.getByRole("button", { name: "T-2 をオーケストレータへ引用" }));
    await user.click(screen.getByRole("button", { name: "T-1 をオーケストレータへ引用" }));
    expect(screen.getAllByRole("button", { name: /の引用を外す$/ })).toHaveLength(2);
    await user.click(screen.getByRole("button", { name: "T-1 の引用を外す" }));
    const box = screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" });
    await user.type(box, "why is this slow?{Enter}");
    await waitFor(() => expect(transport.callsOf("send_user_message")).toHaveLength(1));
    expect(transport.callsOf("send_user_message")[0].text).toBe("@T-2 second task\n\nwhy is this slow?");
    await waitFor(() => expect(screen.queryAllByRole("button", { name: /の引用を外す$/ })).toHaveLength(0));
  });

  it("switches to the runs view, which is honestly empty", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "履歴" }));
    expect(screen.getByText("現状は何もありません")).toBeInTheDocument();
    expect(screen.queryByRole("complementary", { name: "projects" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "コーディング" }));
    expect(screen.getByRole("complementary", { name: "projects" })).toBeInTheDocument();
  });

  it("shows the selected task's agent output in the bottom panel and goes back to git", async () => {
    const { store, user } = setup();
    await open(store);
    await user.click(screen.getByRole("button", { name: "add hello.txt" }));
    const output = screen.getByRole("region", { name: "agent output" });
    expect(within(output).getByText(/^Looks good\./)).toBeInTheDocument();
    expect(within(output).getByLabelText("step results")).toHaveTextContent("implement result:");
    expect(screen.getByRole("button", { name: "T-1 の出力" })).toBeInTheDocument();
    await user.click(within(output).getByRole("button", { name: "出力を閉じる" }));
    expect(screen.getByRole("region", { name: "git" })).toBeInTheDocument();
  });
});

describe("lazy history", () => {
  it("loads recent events first and older pages on demand, ending like a full load", async () => {
    const full = setup({ snapshot: FULL_RUN.end });
    await open(full.store);
    const expected = full.store.getState().project?.transcripts;
    full.store.stop();
    document.body.innerHTML = "";

    const { store, user, transport } = setup({ snapshot: FULL_RUN.end, store: { historyWindow: 10, historyPage: 7 } });
    await open(store);
    const view = () => store.getState().project;
    expect(view()?.historyStart).toBe(LAST - 9);
    expect(transport.callsOf("list_events")[0].after_seq).toBe(LAST - 10);
    const older = await screen.findByRole("button", { name: /さらに前の履歴を読み込む/ });
    await user.click(older);
    await waitFor(() => expect(view()?.historyStart).toBe(LAST - 16));
    while ((view()?.historyStart ?? 1) > 1) await act(() => store.loadOlderHistory());
    expect(view()?.transcripts).toEqual(expected);
    expect(view()?.state).toEqual(FULL_RUN.end.state);
    expect(screen.queryByRole("button", { name: /さらに前の履歴を読み込む/ })).not.toBeInTheDocument();
    expect(view()?.transcripts[ORCHESTRATOR]?.some((i) => i.kind === "group")).toBe(true);
  });
});

describe("reopening", () => {
  it("opens the project that was open last after a reload", async () => {
    const prefs = memoryPrefs({ [LAST_PROJECT_KEY]: PROJECT.path });
    const { store, transport } = setup({ store: { prefs } });
    await waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
    expect(transport.callsOf("open_project")[0].path).toBe(PROJECT.path);
  });

  it("does not open a remembered project the core does not know", async () => {
    const prefs = memoryPrefs({ [LAST_PROJECT_KEY]: "/somewhere/else" });
    const { store, transport } = setup({ store: { prefs } });
    await waitFor(() => expect(transport.callsOf("list_projects")).toHaveLength(1));
    expect(transport.callsOf("open_project")).toHaveLength(0);
    expect(store.getState().project).toBeNull();
  });

  it("remembers the opened project", async () => {
    const prefs = memoryPrefs();
    const { store } = setup({ store: { prefs } });
    await open(store);
    expect(prefs.get(LAST_PROJECT_KEY)).toBe(PROJECT.path);
  });
});

describe("git refresh", () => {
  it("re-reads git after a domain change", async () => {
    const { store, transport } = setup({ snapshot: FULL_RUN.end });
    await open(store);
    const before = transport.callsOf("get_git_overview").length;
    act(() => transport.emit(ev(LAST + 1, { type: "domain", event: { type: "task_status_changed", task: "T-1", status: "done" } })));
    await waitFor(() => expect(transport.callsOf("get_git_overview").length).toBe(before + 1));
  });
});
