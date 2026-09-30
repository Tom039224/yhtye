// The BRANCHES tree and the chat-aware panels (Stage 8b, orchestrator-desktop §9).

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../App";
import { CommandError } from "../api/transport";
import { AppStore, chatPrefKey } from "../store/app";
import { memoryPrefs, type Prefs } from "../store/prefs";
import { StoreContext } from "../store/useStore";
import { CHAT_GIT, CHAT_LOG, chatSnapshot } from "../test/chats";
import { orchestratorKey } from "../store/chats";
import { ev, prompted } from "../test/events";
import { PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";

function setup(opts: { prefs?: Prefs; empty?: boolean } = {}) {
  const transport = new MemoryTransport();
  const core = new FakeCore(PROJECT, chatSnapshot());
  core.log = CHAT_LOG;
  core.git = CHAT_GIT;
  if (opts.empty) {
    core.snapshot = chatSnapshot([], { ...core.snapshot.state, chats: [], groups: [], tasks: [] });
    core.log = [];
  }
  core.attach(transport);
  const prefs = opts.prefs ?? memoryPrefs();
  const store = new AppStore(transport, { prefs });
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <App />
    </StoreContext.Provider>,
  );
  return { transport, core, store, prefs, user: userEvent.setup() };
}

async function open(store: AppStore) {
  await act(() => store.openProject(PROJECT.path));
  await waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
  await screen.findByRole("list", { name: "branches" });
  await waitFor(() => expect(store.getState().git?.overview).not.toBeNull());
}

const tree = () => screen.getByRole("list", { name: "branches" });
const conversation = () => screen.getByRole("region", { name: "orchestrator" });
const tasks = () => screen.getByRole("region", { name: "tasks" });

describe("BRANCHES tree", () => {
  it("lists local branches with their chats newest first and hides Yhtye's internal branches", async () => {
    const { store } = setup();
    await open(store);
    const names = within(tree()).getAllByRole("button", { expanded: true }).map((b) => b.textContent);
    expect(names.join(" ")).toContain("main");
    expect(names.join(" ")).toContain("feat/x");
    expect(within(tree()).queryByText(/yhtye\//)).not.toBeInTheDocument();
    const rows = within(tree()).getAllByRole("button").map((b) => b.textContent ?? "");
    expect(rows.some((r) => r.includes("Add feature x"))).toBe(true);
    expect(rows.some((r) => r.includes("Refactor checkout"))).toBe(true);
    // The chat's age is shown (3m / 5h).
    expect(within(tree()).getByRole("button", { name: /Add feature x/ })).toHaveTextContent("3m");
    expect(within(tree()).getByRole("button", { name: /Refactor checkout/ })).toHaveTextContent("5h");
  });

  it("collapses and expands a branch without selecting its chat", async () => {
    const { store, user } = setup();
    await open(store);
    const main = within(tree()).getByRole("button", { name: "main" });
    await user.click(main);
    expect(main).toHaveAttribute("aria-expanded", "false");
    expect(within(tree()).queryByRole("button", { name: /Refactor checkout/ })).not.toBeInTheDocument();
    expect(screen.getByTestId("chat-title")).toHaveTextContent("Add feature x");
    await user.click(main);
    expect(within(tree()).getByRole("button", { name: /Refactor checkout/ })).toBeInTheDocument();
  });

  it("shows chats of a branch git no longer has under a deleted-branches heading", async () => {
    const { store, core, user } = setup();
    core.git = { ...CHAT_GIT, branches: CHAT_GIT.branches.filter((b) => b.name !== "feat/x") };
    await open(store);
    expect(within(tree()).getByText("(削除されたブランチ)")).toBeInTheDocument();
    await user.click(within(tree()).getByRole("button", { name: /Add feature x/ }));
    expect(screen.getByTestId("chat-branch")).toHaveTextContent("削除済み");
    expect(screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" })).toBeDisabled();
    expect(within(conversation()).getByText(/存在しないため送信できません/)).toBeInTheDocument();
    // The history is still readable.
    expect(within(conversation()).getByText("Reply about feature")).toBeInTheDocument();
  });

  it("moves a chat under its renamed branch, updates the header and notes it in the conversation", async () => {
    const { store, core, transport } = setup();
    await open(store);
    expect(screen.getByTestId("chat-branch")).toHaveTextContent("feat/x");
    // git has the new name by the time the tree re-reads it.
    core.git = { ...CHAT_GIT, branches: CHAT_GIT.branches.map((b) => (b.name === "feat/x" ? { ...b, name: "feat/renamed" } : b)) };
    act(() =>
      transport.emit(
        ev(5, { type: "domain", event: { type: "chat_branch_changed", chat: "C-2", from: "feat/x", to: "feat/renamed" } }),
      ),
    );
    expect(screen.getByTestId("chat-branch")).toHaveTextContent("feat/renamed");
    expect(screen.getByTestId("head-branch")).toHaveTextContent("feat/renamed");
    expect(within(conversation()).getByText("ブランチ名が feat/x → feat/renamed に変わりました")).toBeInTheDocument();
    await waitFor(() => expect(within(tree()).getByRole("button", { name: "feat/renamed の新しいチャット" })).toBeInTheDocument());
    expect(within(tree()).queryByRole("button", { name: "feat/x の新しいチャット" })).not.toBeInTheDocument();
    expect(within(tree()).getByRole("button", { name: /Add feature x/ })).toBeInTheDocument();
    expect(within(tree()).queryByText("(削除されたブランチ)")).not.toBeInTheDocument();
  });

  it("marks the main worktree's branch and a running chat", async () => {
    const { store, transport } = setup();
    await open(store);
    const main = within(tree()).getByRole("button", { name: "main" });
    expect(main.querySelector(".dot-ok")).not.toBeNull();
    expect(screen.getByTestId("chat-ring-C-2")).toHaveClass("ring-stopped");
    // The orchestrator of C-2 starts (ready), then works (running); its branch spins too.
    act(() => {
      transport.emit(
        ev(5, {
          type: "session_started", session: orchestratorKey("C-2"), role: "orchestrator", task: null,
          pid: 1, acp_session_id: "a", resumed: false, agent: null,
        }),
      );
    });
    expect(screen.getByTestId("chat-ring-C-2")).toHaveClass("ring-ready");
    act(() => transport.emit(prompted(6, orchestratorKey("C-2"))));
    expect(screen.getByTestId("chat-ring-C-2")).toHaveClass("ring-running");
    expect(within(tree()).getByRole("button", { name: /feat\/x$/ }).querySelector(".ring-running")).not.toBeNull();
  });
});

describe("selecting a chat", () => {
  it("switches the conversation, the header, the title bar and the tasks column", async () => {
    const { store, user } = setup();
    await open(store);
    // Opens the last used chat.
    expect(screen.getByTestId("chat-title")).toHaveTextContent("Add feature x");
    expect(screen.getByTestId("chat-branch")).toHaveTextContent("feat/x");
    expect(screen.getByTestId("head-branch")).toHaveTextContent("feat/x");
    expect(within(conversation()).getByText("Reply about feature")).toBeInTheDocument();
    expect(within(conversation()).queryByText("Reply about checkout")).not.toBeInTheDocument();
    expect(within(tasks()).getByRole("article", { name: "task T-2" })).toBeInTheDocument();
    expect(within(tasks()).queryByRole("article", { name: "task T-1" })).not.toBeInTheDocument();

    await user.click(within(tree()).getByRole("button", { name: /Refactor checkout/ }));
    expect(screen.getByTestId("chat-title")).toHaveTextContent("Refactor checkout");
    expect(screen.getByTestId("head-branch")).toHaveTextContent("main");
    expect(within(conversation()).getByText("Reply about checkout")).toBeInTheDocument();
    expect(within(conversation()).queryByText("Reply about feature")).not.toBeInTheDocument();
    expect(within(tasks()).getByRole("article", { name: "task T-1" })).toBeInTheDocument();
    expect(within(tasks()).queryByRole("article", { name: "task T-2" })).not.toBeInTheDocument();
    // The git panel stays repository-wide.
    expect(screen.getByRole("region", { name: "git" })).toBeInTheDocument();
  });

  it("does not carry a draft over to another chat", async () => {
    const { store, user, transport } = setup();
    await open(store);
    const box = () => screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" });
    await user.type(box(), "draft for feat/x");
    await user.click(within(tree()).getByRole("button", { name: /Refactor checkout/ }));
    expect(box()).toHaveValue("");
    await user.type(box(), "{Enter}");
    expect(transport.callsOf("send_user_message")).toHaveLength(0);
  });

  it("sends to the selected chat and remembers the choice for the project", async () => {
    const { store, user, transport, prefs } = setup();
    await open(store);
    await user.click(within(tree()).getByRole("button", { name: /Refactor checkout/ }));
    await user.type(screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" }), "ship it{Enter}");
    await waitFor(() => expect(transport.callsOf("send_user_message")).toHaveLength(1));
    expect(transport.callsOf("send_user_message")[0]).toMatchObject({ chat: "C-1", text: "ship it" });
    expect(prefs.get(chatPrefKey("repo"))).toBe("C-1");
  });

  it("marks a chat that got a notification while another one is shown, until it is opened", async () => {
    const { store, user, transport } = setup();
    await open(store);
    act(() =>
      transport.emit(
        ev(5, {
          type: "domain",
          event: { type: "inbox_queued", entry: { id: 7, chat: "C-1", item: { kind: "help_raised", attrs: [], body: "stuck" } } },
        }),
      ),
    );
    const row = within(tree()).getByRole("button", { name: /Refactor checkout/ });
    expect(within(row).getByRole("img", { name: "未読の通知" })).toBeInTheDocument();
    await user.click(row);
    expect(within(row).queryByRole("img", { name: "未読の通知" })).not.toBeInTheDocument();
  });
});

describe("creating chats and branches", () => {
  it("creates a chat on a branch from its row and selects it", async () => {
    const { store, user, transport } = setup();
    await open(store);
    await user.click(within(tree()).getByRole("button", { name: "main の新しいチャット" }));
    expect(transport.callsOf("create_chat")).toEqual([{ type: "create_chat", project: "repo", branch: "main" }]);
    await waitFor(() => expect(screen.getByTestId("head-branch")).toHaveTextContent("main"));
    expect(screen.getByTestId("chat-title")).toHaveTextContent("新しいチャット");
    expect(screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" })).toBeEnabled();
  });

  it("creates a branch with the default start point and opens its first chat", async () => {
    const { store, user, transport } = setup();
    await open(store);
    await user.click(screen.getByRole("button", { name: "新しいブランチ" }));
    const form = screen.getByRole("form", { name: "新しいブランチ" });
    expect(within(form).getByRole("option", { name: "HEAD (main)" })).toBeInTheDocument();
    expect(within(form).queryByRole("option", { name: /yhtye\// })).not.toBeInTheDocument();
    expect(within(form).getByRole("button", { name: "作成" })).toBeDisabled();
    await user.type(within(form).getByRole("textbox", { name: "ブランチ名" }), "feat/new{Enter}");
    await waitFor(() => expect(transport.callsOf("create_branch")).toHaveLength(1));
    expect(transport.callsOf("create_branch")[0]).toEqual({ type: "create_branch", project: "repo", name: "feat/new", from: undefined });
    await waitFor(() => expect(screen.queryByRole("form", { name: "新しいブランチ" })).not.toBeInTheDocument());
    expect(screen.getByTestId("chat-branch")).toHaveTextContent("feat/new");
    // The new branch is in the tree (git was re-read) with its chat.
    expect(await within(tree()).findByRole("button", { name: "feat/new の新しいチャット" })).toBeInTheDocument();
  });

  it("closes the new-branch form when another project is opened", async () => {
    const { store, user, core } = setup();
    await open(store);
    await user.click(screen.getByRole("button", { name: "新しいブランチ" }));
    expect(screen.getByRole("form", { name: "新しいブランチ" })).toBeInTheDocument();
    core.info = { ...core.info, id: "other", path: "/other" };
    await act(() => store.openProject("/other"));
    await waitFor(() => expect(store.getState().project?.info.id).toBe("other"));
    expect(screen.queryByRole("form", { name: "新しいブランチ" })).not.toBeInTheDocument();
  });

  it("passes a chosen start point", async () => {
    const { store, user, transport } = setup();
    await open(store);
    await user.click(screen.getByRole("button", { name: "新しいブランチ" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "開始点" }), "feat/x");
    await user.type(screen.getByRole("textbox", { name: "ブランチ名" }), "topic{Enter}");
    await waitFor(() => expect(transport.callsOf("create_branch")).toHaveLength(1));
    expect(transport.callsOf("create_branch")[0]).toMatchObject({ name: "topic", from: "feat/x" });
  });

  it("shows a refusal in the form, keeps it open and closes it with Escape", async () => {
    const { store, user, core } = setup();
    await open(store);
    core.branchError = new CommandError("conflict", "branch feat/x already exists");
    await user.click(screen.getByRole("button", { name: "新しいブランチ" }));
    await user.type(screen.getByRole("textbox", { name: "ブランチ名" }), "feat/x{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("already exists");
    expect(screen.getByRole("form", { name: "新しいブランチ" })).toBeInTheDocument();
    expect(screen.getByTestId("chat-title")).toHaveTextContent("Add feature x");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("form", { name: "新しいブランチ" })).not.toBeInTheDocument();
  });
});

describe("a project without chats", () => {
  it("shows the hint instead of a conversation and does not start anything", async () => {
    const { store, transport } = setup({ empty: true });
    await open(store);
    expect(screen.getByTestId("no-chat")).toHaveTextContent("BRANCHES の「+ 新しいチャット」から始めます");
    expect(screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" })).toBeDisabled();
    expect(transport.callsOf("create_chat")).toHaveLength(0);
    // The title bar falls back to the main worktree's branch.
    expect(screen.getByTestId("head-branch")).toHaveTextContent("main");
    expect(within(tasks()).getByText(/チャットを選ぶと/)).toBeInTheDocument();
  });

  it("expands the branch checked out in the main clone by default so + 新しいチャット is visible", async () => {
    const { store, user, transport } = setup({ empty: true });
    await open(store);
    expect(within(tree()).getByRole("button", { name: "main" })).toHaveAttribute("aria-expanded", "true");
    expect(within(tree()).getByRole("button", { name: "feat/x" })).toHaveAttribute("aria-expanded", "false");
    await user.click(within(tree()).getByRole("button", { name: "main の新しいチャット" }));
    expect(transport.callsOf("create_chat")).toEqual([{ type: "create_chat", project: "repo", branch: "main" }]);
  });

  it("offers + 新しいチャット under each branch", async () => {
    const { store, user, transport } = setup({ empty: true });
    await open(store);
    await user.click(within(tree()).getByRole("button", { name: "feat/x" }));
    await user.click(within(tree()).getByRole("button", { name: "feat/x の新しいチャット" }));
    expect(transport.callsOf("create_chat")).toEqual([{ type: "create_chat", project: "repo", branch: "feat/x" }]);
    await waitFor(() => expect(screen.getByTestId("chat-title")).toHaveTextContent("新しいチャット"));
  });
});

describe("restoring the selection", () => {
  it("reopens the chat remembered for the project", async () => {
    const prefs = memoryPrefs({ [chatPrefKey("repo")]: "C-1" });
    const { store } = setup({ prefs });
    await open(store);
    expect(screen.getByTestId("chat-title")).toHaveTextContent("Refactor checkout");
  });
});
