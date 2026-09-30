import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "./App";
import type { Snapshot, State } from "./api/generated";
import { CommandError } from "./api/transport";
import { AppStore } from "./store/app";
import { emptyState } from "./store/domain";
import { ORCHESTRATOR } from "./test/events";
import { StoreContext } from "./store/useStore";
import { agentText, chunk, delivered, prompted, turnEnded, userMessage } from "./test/events";
import { durable, FULL_RUN, PROJECT } from "./test/fixtures";
import { FakeCore, MemoryTransport } from "./test/memoryTransport";

const LAST = durable(FULL_RUN).at(-1)?.seq ?? 0;

function setup(opts: { snapshot?: Snapshot; log?: typeof FULL_RUN.events; connected?: boolean } = {}) {
  const transport = new MemoryTransport(opts.connected === false ? { state: "connecting" } : { state: "open" });
  const core = new FakeCore(PROJECT, opts.snapshot ?? FULL_RUN.start);
  core.log = opts.log ?? durable(FULL_RUN);
  core.attach(transport);
  const store = new AppStore(transport);
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <App />
    </StoreContext.Provider>,
  );
  return { transport, core, store, user: userEvent.setup() };
}

async function openProject(store: AppStore) {
  await act(() => store.openProject(PROJECT.path));
}

function emptySnapshot(state?: Partial<State>): Snapshot {
  // The recordings' chat `C-1` (on `main`) exists and is selected.
  const chat = { id: "C-1", worktree: "/repo", title: null };
  const info = { ...chat, created_ms: 1, last_used_ms: 1 };
  return { seq: 0, state: { ...emptyState("repo", { max_review_rounds: 2 }), chats: [chat], ...state }, sessions: [], chats: [info] };
}

describe("App", () => {
  it("shows a lost connection in the banner and keeps the composer disabled while disconnected", async () => {
    const { transport, store } = setup({ connected: false });
    expect(screen.queryByTestId("connection-lost")).not.toBeInTheDocument();
    act(() => transport.setStatus({ state: "closed", reason: "connection refused", retryInMs: 1000 }));
    expect(screen.getByTestId("connection-lost")).toHaveTextContent("connection refused");
    expect(screen.getByText("プロジェクト (git リポジトリ) を開いてください。")).toBeInTheDocument();
    act(() => transport.setStatus({ state: "open" }));
    await openProject(store);
    act(() => transport.setStatus({ state: "closed", reason: "bridge stopped", retryInMs: 500 }));
    expect(screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" })).toBeDisabled();
    expect(screen.getByText("コアに接続していません")).toBeInTheDocument();
  });

  it("opens a project from the form and renders the recorded run", async () => {
    const { user, transport } = setup();
    await user.click(screen.getByRole("button", { name: "プロジェクトを開く" }));
    await user.type(screen.getByPlaceholderText("/path/to/git/repository"), "/tmp/repo");
    await user.click(screen.getByRole("button", { name: "開く" }));
    const conversation = await screen.findByRole("region", { name: "orchestrator" });
    await within(conversation).findByText(/^I will create a group for this\./);
    expect(transport.callsOf("open_project")[0].path).toBe("/tmp/repo");
    // The chat's title (its first message) heads the conversation and names its row.
    expect(within(conversation).getByTestId("chat-title")).toHaveTextContent("Add hello.txt please");
    expect(within(conversation).getByText(/^I will create a group for this\./)).toBeInTheDocument();
    expect(within(conversation).getByText(/Done: hello.txt is on main\./)).toBeInTheDocument();
    const thought = within(conversation).getAllByText("thought")[0].closest("details");
    expect(thought).not.toHaveAttribute("open");
    expect(within(conversation).getByText(/Read README.md/).closest(".tool")).toHaveTextContent("completed");

    const tasks = screen.getByRole("region", { name: "tasks" });
    const task = within(tasks).getByRole("article", { name: "task T-1" });
    expect(within(task).getByText("完了")).toBeInTheDocument();
    expect(within(task).queryByRole("button", { name: "T-1 を中止" })).not.toBeInTheDocument();
    const steps = within(within(task).getByLabelText("steps")).getAllByRole("listitem", { hidden: false });
    expect(steps.map((s) => s.getAttribute("aria-label"))).toEqual(["implement done", "review done (approve)", "done done"]);
    expect(within(tasks).getByRole("region", { name: "group G-1" })).toHaveTextContent("1/1 完了");
  });

  it("shows a task's agent output when its title is clicked", async () => {
    const { user, store } = setup();
    await openProject(store);
    await user.click(screen.getByRole("button", { name: "add hello.txt" }));
    const output = screen.getByRole("region", { name: "agent output" });
    expect(within(output).getByText(/^Looks good\./)).toBeInTheDocument();
    await user.click(within(output).getByRole("button", { name: /T-1\/implementer/ }));
    expect(within(output).getByText(/^Writing the file\./)).toBeInTheDocument();
    expect(within(output).getByText(/Write hello.txt/)).toBeInTheDocument();
  });

  it("sends with Enter, adds a newline with Shift+Enter, and keeps the text on failure", async () => {
    const { user, store, transport, core } = setup();
    await openProject(store);
    const box = screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" });
    await user.type(box, "line one{Shift>}{Enter}{/Shift}line two");
    expect(box).toHaveValue("line one\nline two");
    expect(transport.callsOf("send_user_message")).toHaveLength(0);
    await user.type(box, "{Enter}");
    await waitFor(() => expect(box).toHaveValue(""));
    expect(transport.callsOf("send_user_message")[0].text).toBe("line one\nline two");

    core.failures.set("send_user_message", new CommandError("unavailable", "the project's orchestration has stopped"));
    await user.type(box, "again{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("orchestration has stopped");
    expect(box).toHaveValue("again");
  });

  it("streams text, marks queued messages and offers cancel during a turn", async () => {
    const { user, store, transport } = setup();
    await openProject(store);
    act(() => {
      transport.emit(userMessage(LAST + 1, 9, "one more thing"));
      transport.emit(prompted(LAST + 2, ORCHESTRATOR));
    });
    expect(screen.getByText(/待機中/)).toBeInTheDocument();
    expect(screen.getByTestId("orchestrator-status")).toHaveTextContent("working");
    expect(screen.getByTestId("orchestrator-status")).toHaveClass("status-dot-working");
    act(() => {
      transport.emit({ ...chunk(LAST + 2, ORCHESTRATOR, "Work"), seq: LAST + 2 });
      transport.emit({ ...chunk(LAST + 2, ORCHESTRATOR, "ing on it"), seq: LAST + 2 });
    });
    expect(screen.getByTestId("streaming")).toHaveTextContent("Working on it");

    await user.click(screen.getByRole("button", { name: "ターンを中止" }));
    expect(transport.callsOf("cancel_orchestrator_turn")).toHaveLength(1);

    act(() => {
      transport.emit(agentText(LAST + 3, ORCHESTRATOR, "Working on it"));
      transport.emit(delivered(LAST + 4, 9));
      transport.emit(turnEnded(LAST + 5, ORCHESTRATOR, "cancelled"));
    });
    expect(screen.queryByTestId("streaming")).not.toBeInTheDocument();
    expect(screen.getAllByText("Working on it")).toHaveLength(1);
    expect(screen.queryByText(/待機中/)).not.toBeInTheDocument();
    expect(screen.getByText("turn ended: cancelled")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "ターンを中止" })).not.toBeInTheDocument();
  });

  it("shows help, interrupted and merge_blocked indicators and cancels tasks and groups", async () => {
    const base = FULL_RUN.end.state;
    const task = base.tasks[0];
    const snapshot = emptySnapshot({
      groups: [{ ...base.groups[0], status: "merge_blocked", detail: "main has uncommitted changes" }],
      tasks: [
        { ...task, status: "handling" },
        { ...task, id: "T-2", title: "second", status: "interrupted" },
      ],
      helps: [
        {
          id: "H-1", task: "T-1", step: 0, kind: "merge_conflict", message: "conflict in a.txt",
          source: { by: "yhtye" }, state: "open", agent_lost: false, reply: null,
        },
      ],
    });
    const { user, store, transport } = setup({ snapshot, log: [] });
    await openProject(store);
    const tasks = screen.getByRole("region", { name: "tasks" });
    expect(within(tasks).getByText(/マージが保留されています: main has uncommitted changes/)).toBeInTheDocument();
    expect(within(tasks).getByText(/マージコンフリクト/)).toBeInTheDocument();
    expect(within(tasks).getByText("conflict in a.txt")).toBeInTheDocument();
    expect(within(tasks).getByText(/再起動で中断されました/)).toBeInTheDocument();
    expect(within(tasks).getAllByText("対処中")).toHaveLength(2);

    await user.click(within(screen.getByRole("article", { name: "task T-2" })).getByRole("button", { name: "T-2 を中止" }));
    await user.click(within(tasks).getByRole("button", { name: "グループを中止" }));
    await user.click(within(tasks).getByRole("button", { name: "マージを再試行" }));
    expect(transport.callsOf("cancel_task")[0].task).toBe("T-2");
    expect(transport.callsOf("cancel_group")[0].group).toBe("G-1");
    expect(transport.callsOf("retry_group_merge")[0].group).toBe("G-1");
  });

  it("shows empty states for a new project", async () => {
    const { store } = setup({ snapshot: emptySnapshot(), log: [] });
    await openProject(store);
    expect(screen.getByText(/会話はまだありません/)).toBeInTheDocument();
    expect(screen.getByText(/グループはまだありません/)).toBeInTheDocument();
    expect(screen.getByText("コミットはまだありません。")).toBeInTheDocument();
    expect(screen.getByText("not started")).toBeInTheDocument();
    // The fake core reports no usage: honest placeholders, no numbers.
    expect(screen.getByTestId("usage-5h")).toHaveTextContent("5h—");
    expect(screen.queryByTestId("wait-banner")).not.toBeInTheDocument();
  });
});
