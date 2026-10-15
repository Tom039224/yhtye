// The BRANCHES tree of worktrees and the chat-aware panels (Stage 8b / 8e,
// orchestrator-desktop §9).

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import App from "../App";
import { CommandError } from "../api/transport";
import { AppStore, chatPrefKey } from "../store/app";
import { memoryPrefs, type Prefs } from "../store/prefs";
import { StoreContext } from "../store/useStore";
import { CHAT_GIT, CHAT_LOG, chatSnapshot, FEAT_WT, MAIN_WT } from "../test/chats";
import { orchestratorKey } from "../store/chats";
import { ev, prompted, turnEnded } from "../test/events";
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
  await waitFor(() => expect(store.getState().git?.statuses).not.toBeNull());
}

const tree = () => screen.getByRole("list", { name: "branches" });
/** The button of a chat's row (its "⋯" menu button is named after the chat too). */
const chatButton = (title: RegExp) =>
  within(tree()).getByRole("button", { name: (name) => !name.endsWith("の操作") && title.test(name) });
const conversation = () => screen.getByRole("region", { name: "orchestrator" });
const tasks = () => screen.getByRole("region", { name: "tasks" });

/** A branch's status: its tone class and the glyph inside it. */
function expectStatus(status: HTMLElement, tone: string, icon: string) {
  expect(status).toHaveClass("branch-status", tone);
  expect(status.querySelector(`svg[data-icon="${icon}"]`)).not.toBeNull();
}

describe("BRANCHES tree", () => {
  it("lists the worktrees by their branch with their chats newest first and hides Yhtye's internal ones", async () => {
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
    expect(chatButton(/Add feature x/)).toHaveTextContent("3m");
    expect(chatButton(/Refactor checkout/)).toHaveTextContent("5h");
  });

  it("collapses and expands a worktree without selecting its chat", async () => {
    const { store, user } = setup();
    await open(store);
    const main = within(tree()).getByRole("button", { name: "main" });
    await user.click(main);
    expect(main).toHaveAttribute("aria-expanded", "false");
    expect(within(tree()).queryByRole("button", { name: /Refactor checkout/ })).not.toBeInTheDocument();
    expect(screen.getByTestId("chat-title")).toHaveTextContent("Add feature x");
    await user.click(main);
    expect(chatButton(/Refactor checkout/)).toBeInTheDocument();
  });

  it("shows chats of a worktree git no longer lists under a not-found heading, readable but not sendable", async () => {
    const { store, core, user } = setup();
    core.git = { ...CHAT_GIT, worktrees: CHAT_GIT.worktrees.filter((w) => w.path !== FEAT_WT) };
    await open(store);
    expect(within(tree()).getByText("(見つからない作業ツリー)")).toBeInTheDocument();
    await user.click(chatButton(/Add feature x/));
    expect(screen.getByTestId("chat-branch")).toHaveTextContent("作業ツリーが見つかりません");
    expect(screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" })).toBeDisabled();
    expect(within(conversation()).getByText(/見つからないため送信できません/)).toBeInTheDocument();
    // The history is still readable.
    expect(within(conversation()).getByText("Reply about feature")).toBeInTheDocument();
  });

  it("names a worktree by what it has checked out now: a rename outside Yhtye shows up on focus", async () => {
    const { store, core } = setup();
    await open(store);
    expect(screen.getByTestId("chat-branch")).toHaveTextContent("feat/x");
    const renamed = (w: (typeof CHAT_GIT.worktrees)[number]) => (w.path === FEAT_WT ? { ...w, branch: "feat/renamed" } : w);
    core.git = {
      ...CHAT_GIT,
      branches: CHAT_GIT.branches.map((b) => (b.name === "feat/x" ? { ...b, name: "feat/renamed" } : b)),
      worktrees: CHAT_GIT.worktrees.map(renamed),
    };
    fireEvent.focus(window);
    await waitFor(() => expect(screen.getByTestId("chat-branch")).toHaveTextContent("feat/renamed"));
    expect(screen.getByTestId("head-branch")).toHaveTextContent("feat/renamed");
    expect(within(tree()).getByRole("button", { name: "feat/renamed の新しいチャット" })).toBeInTheDocument();
    expect(within(tree()).queryByRole("button", { name: "feat/x の新しいチャット" })).not.toBeInTheDocument();
    // The chat stays with its worktree; nothing is said in the conversation.
    expect(chatButton(/Add feature x/)).toBeInTheDocument();
    expect(within(conversation()).queryByText(/ブランチ名が/)).not.toBeInTheDocument();
    expect(within(tree()).queryByText("(見つからない作業ツリー)")).not.toBeInTheDocument();
  });

  it("re-reads git when an orchestrator's turn ends and shows a detached worktree by its commit", async () => {
    const { store, core, transport } = setup();
    await open(store);
    const reads = transport.callsOf("get_git_overview").length;
    const detached = (w: (typeof CHAT_GIT.worktrees)[number]) => (w.path === FEAT_WT ? { ...w, branch: null, head_sha: "abcdef1234" } : w);
    core.git = { ...CHAT_GIT, worktrees: CHAT_GIT.worktrees.map(detached) };
    act(() => transport.emit({ ...turnEnded(4, orchestratorKey("C-2")), live: true }));
    await waitFor(() => expect(transport.callsOf("get_git_overview").length).toBeGreaterThan(reads));
    await waitFor(() => expect(screen.getByTestId("chat-branch")).toHaveTextContent("detached @abcdef1"));
    expect(within(tree()).getByRole("button", { name: "detached @abcdef1 の新しいチャット" })).toBeInTheDocument();
  });

  it("lists branches checked out nowhere under 他のブランチ and starts a chat there", async () => {
    const { store, core, user, transport } = setup();
    core.git = { ...CHAT_GIT, branches: [...CHAT_GIT.branches, { name: "old", sha: "o".repeat(40) }] };
    await open(store);
    const other = within(tree()).getByRole("button", { name: /他のブランチ \(1\)/ });
    expect(other).toHaveAttribute("aria-expanded", "false");
    await user.click(other);
    await user.click(within(tree()).getByRole("button", { name: "old の新しいチャット" }));
    expect(transport.callsOf("create_chat")).toEqual([{ type: "create_chat", project: "repo", branch: "old" }]);
  });

  it("marks the branches with their PR/merge status while a running chat spins in its own row", async () => {
    const { store, transport, core } = setup();
    core.branchStatuses = {
      default_branch: "main",
      statuses: [{ branch: "feat/x", state: "pr_open", pr_number: 7, pr_url: "https://example.test/u/repo/pull/7" }],
    };
    await open(store);
    // The default branch gets the neutral branch icon (no state).
    const main = within(tree()).getByRole("button", { name: "main" });
    expect(main.querySelector('.branch-status [data-icon="git-branch"]')).not.toBeNull();
    expect(main.querySelector('.branch-status[class*="branch-status-"]')).toBeNull();
    // The branch with an open PR shows its status, not a ring.
    const feat = within(tree()).getByRole("button", { name: /feat\/x$/ });
    expectStatus(within(feat).getByRole("img", { name: "PR 作成済み (#7)" }), "branch-status-open", "git-pull-request");
    expect(feat.querySelector(".ring-running")).toBeNull();
    expect(screen.getByTestId("chat-ring-C-2")).toHaveClass("ring-stopped");
    // The orchestrator of C-2 starts (ready), then works (running); its branch row keeps the status.
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
    const again = within(tree()).getByRole("button", { name: /feat\/x$/ });
    expect(again.querySelector(".ring-running")).toBeNull();
    expect(within(again).getByRole("img", { name: "PR 作成済み (#7)" })).toBeInTheDocument();
  });

  it("shows every PR/merge state with its Japanese label", async () => {
    const { store, core, user } = setup();
    const sha = (c: string) => c.repeat(40);
    core.git = {
      ...CHAT_GIT,
      branches: [
        ...CHAT_GIT.branches,
        { name: "old", sha: sha("o") },
        { name: "ship", sha: sha("s") },
        { name: "redo", sha: sha("r") },
        { name: "stale", sha: sha("t") },
      ],
      worktrees: [
        ...CHAT_GIT.worktrees,
        { path: "/wt/old", branch: "old", head_sha: sha("o"), is_main: false, missing: false },
        { path: "/wt/ship", branch: "ship", head_sha: sha("s"), is_main: false, missing: false },
        { path: "/wt/redo", branch: "redo", head_sha: sha("r"), is_main: false, missing: false },
      ],
    };
    core.branchStatuses = {
      default_branch: "main",
      statuses: [
        { branch: "feat/x", state: "pr_open", pr_number: 7, pr_url: "https://example.test/u/repo/pull/7" },
        { branch: "old", state: "unmerged", pr_number: null, pr_url: null },
        { branch: "ship", state: "merged", pr_number: 9, pr_url: "https://example.test/u/repo/pull/9" },
        { branch: "redo", state: "merged_with_changes", pr_number: 8, pr_url: "https://example.test/u/repo/pull/8" },
        { branch: "stale", state: "unmerged", pr_number: null, pr_url: null },
      ],
    };
    await open(store);
    expectStatus(within(tree()).getByRole("img", { name: "PR 作成済み (#7)" }), "branch-status-open", "git-pull-request");
    const merged = within(tree()).getByRole("img", { name: "PR でマージ済み (#9)" });
    expectStatus(merged, "branch-status-merged", "git-merge");
    expect(merged.querySelector(".branch-status-badge")).toBeNull();
    // Merged and then changed: the merge icon with a badge.
    const changed = within(tree()).getByRole("img", { name: "PR でマージ済み・未マージの変更あり (#8)" });
    expectStatus(changed, "branch-status-changes", "git-merge");
    expect(changed.querySelector(".branch-status-badge")).not.toBeNull();
    expect(changed).toHaveAttribute("title", "PR でマージ済み・未マージの変更あり (#8)");
    expectStatus(within(tree()).getByRole("img", { name: "まだ取り込まれていません" }), "branch-status-unmerged", "git-branch");
    // No entry for `main`'s default branch is neutral; a branch with no entry is unknown.
    const main = within(tree()).getByRole("button", { name: "main" });
    expect(within(main).queryByRole("img")).not.toBeInTheDocument();
    // The other branches show their status too.
    await user.click(within(tree()).getByRole("button", { name: /他のブランチ \(1\)/ }));
    const other = within(tree()).getByRole("list", { name: "other branches" });
    expectStatus(within(other).getByRole("img", { name: "まだ取り込まれていません" }), "branch-status-unmerged", "git-branch");
  });

  it("reads an unknown branch as 状態不明 and reuses the statuses until they are stale", async () => {
    const { store, core, transport } = setup();
    const sha = (c: string) => c.repeat(40);
    core.git = {
      ...CHAT_GIT,
      worktrees: [...CHAT_GIT.worktrees, { path: "/wt/mystery", branch: "mystery", head_sha: sha("m"), is_main: false, missing: false }],
    };
    await open(store);
    expect(transport.callsOf("get_branch_statuses")).toHaveLength(1);
    const mystery = within(tree()).getByRole("button", { name: /mystery$/ });
    expectStatus(within(mystery).getByRole("img", { name: "状態不明" }), "branch-status-unknown", "help");
    // A re-read of the overview (an orchestrator's turn ended) does not ask `gh` again.
    const reads = transport.callsOf("get_git_overview").length;
    act(() => transport.emit({ ...turnEnded(4, orchestratorKey("C-2")), live: true }));
    await waitFor(() => expect(transport.callsOf("get_git_overview").length).toBeGreaterThan(reads));
    await waitFor(() => expect(store.getState().git?.overview).not.toBeNull());
    expect(transport.callsOf("get_branch_statuses")).toHaveLength(1);
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

    await user.click(chatButton(/Refactor checkout/));
    expect(screen.getByTestId("chat-title")).toHaveTextContent("Refactor checkout");
    expect(screen.getByTestId("head-branch")).toHaveTextContent("main");
    expect(within(conversation()).getByText("Reply about checkout")).toBeInTheDocument();
    expect(within(conversation()).queryByText("Reply about feature")).not.toBeInTheDocument();
    expect(within(tasks()).getByRole("article", { name: "task T-1" })).toBeInTheDocument();
    expect(within(tasks()).queryByRole("article", { name: "task T-2" })).not.toBeInTheDocument();
    // The git panel stays repository-wide.
    expect(screen.getByRole("region", { name: "git" })).toBeInTheDocument();
  });

  it("keeps a draft per chat: another chat starts empty and the draft is back on return", async () => {
    const { store, user, transport } = setup();
    await open(store);
    const box = () => screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" });
    await user.type(box(), "draft for feat/x");
    await user.click(chatButton(/Refactor checkout/));
    expect(box()).toHaveValue("");
    await user.type(box(), "{Enter}");
    expect(transport.callsOf("send_user_message")).toHaveLength(0);
    await user.type(box(), "draft for main");
    await user.click(chatButton(/Add feature x/));
    expect(box()).toHaveValue("draft for feat/x");
    await user.click(chatButton(/Refactor checkout/));
    expect(box()).toHaveValue("draft for main");
    expect(transport.callsOf("send_user_message")).toHaveLength(0);
  });

  it("clears the draft of the chat it was sent to, and only that one", async () => {
    const { store, user, transport } = setup();
    await open(store);
    const box = () => screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" });
    await user.type(box(), "other draft");
    await user.click(chatButton(/Refactor checkout/));
    await user.type(box(), "ship it{Enter}");
    await waitFor(() => expect(transport.callsOf("send_user_message")).toHaveLength(1));
    await waitFor(() => expect(box()).toHaveValue(""));
    await user.click(chatButton(/Add feature x/));
    expect(box()).toHaveValue("other draft");
    await user.click(chatButton(/Refactor checkout/));
    expect(box()).toHaveValue("");
  });

  it("sends to the selected chat and remembers the choice for the project", async () => {
    const { store, user, transport, prefs } = setup();
    await open(store);
    await user.click(chatButton(/Refactor checkout/));
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
    const row = chatButton(/Refactor checkout/);
    expect(within(row).getByRole("img", { name: "未読の通知" })).toBeInTheDocument();
    await user.click(row);
    expect(within(row).queryByRole("img", { name: "未読の通知" })).not.toBeInTheDocument();
  });
});

describe("creating chats and branches", () => {
  it("creates a chat in a worktree from its row and selects it", async () => {
    const { store, user, transport } = setup();
    await open(store);
    await user.click(within(tree()).getByRole("button", { name: "main の新しいチャット" }));
    expect(transport.callsOf("create_chat")).toEqual([{ type: "create_chat", project: "repo", worktree: MAIN_WT }]);
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
    expect(screen.getByTestId("no-chat")).toHaveTextContent("サイドバーの「新しいチャット」から始めます");
    expect(screen.getByRole("textbox", { name: "オーケストレータへのメッセージ" })).toBeDisabled();
    expect(transport.callsOf("create_chat")).toHaveLength(0);
    // The title bar falls back to the main worktree's branch.
    expect(screen.getByTestId("head-branch")).toHaveTextContent("main");
    expect(within(tasks()).getByText(/チャットを選ぶと/)).toBeInTheDocument();
  });

  it("expands the main clone by default so + 新しいチャット is visible", async () => {
    const { store, user, transport } = setup({ empty: true });
    await open(store);
    expect(within(tree()).getByRole("button", { name: "main" })).toHaveAttribute("aria-expanded", "true");
    expect(within(tree()).getByRole("button", { name: /feat\/x$/ })).toHaveAttribute("aria-expanded", "false");
    await user.click(within(tree()).getByRole("button", { name: "main の新しいチャット" }));
    expect(transport.callsOf("create_chat")).toEqual([{ type: "create_chat", project: "repo", worktree: MAIN_WT }]);
  });

  it("offers + 新しいチャット under each worktree", async () => {
    const { store, user, transport } = setup({ empty: true });
    await open(store);
    await user.click(within(tree()).getByRole("button", { name: /feat\/x$/ }));
    await user.click(within(tree()).getByRole("button", { name: "feat/x の新しいチャット" }));
    expect(transport.callsOf("create_chat")).toEqual([{ type: "create_chat", project: "repo", worktree: FEAT_WT }]);
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

describe("BRANCHES order", () => {
  /** The labels of the worktree rows, top to bottom. */
  const rows = () => [...tree().querySelectorAll(".wt-head .branch-row .name")].map((n) => n.textContent);
  /** A worktree row's button: its name ends with the branch (a status label may come before). */
  const rowButton = (label: string) => within(tree()).getByRole("button", { name: (name) => name.endsWith(label), expanded: true });
  const head = (label: string) => rowButton(label).closest(".wt-head") as HTMLElement;
  const stubTransfer = () => ({ setData: vi.fn(), effectAllowed: "", dropEffect: "" });

  it("lists the worktrees in the order the core keeps, the ones it does not name after them", async () => {
    const { store, core } = setup();
    core.branchOrder = [FEAT_WT];
    await open(store);
    await waitFor(() => expect(rows()).toEqual(["feat/x", "main"]));
  });

  it("moves a worktree from its right-click menu and shows the new order at once", async () => {
    const { store, user, transport } = setup();
    await open(store);
    expect(rows()).toEqual(["main", "feat/x"]);
    await user.pointer({ keys: "[MouseRight]", target: head("main") });
    const menu = screen.getByRole("menu", { name: "main の操作" });
    expect(within(menu).getByRole("menuitem", { name: "上へ移動" })).toBeDisabled();
    await user.click(within(menu).getByRole("menuitem", { name: "下へ移動" }));
    expect(transport.callsOf("reorder_branches").at(-1)).toMatchObject({ keys: [FEAT_WT, MAIN_WT] });
    await waitFor(() => expect(rows()).toEqual(["feat/x", "main"]));
  });

  it("reorders by dragging a worktree onto another", async () => {
    const { store, transport } = setup();
    await open(store);
    const feat = head("feat/x");
    expect(feat).toHaveAttribute("draggable", "true");
    const dataTransfer = stubTransfer();
    fireEvent.dragStart(feat, { dataTransfer });
    fireEvent.dragOver(head("main"), { dataTransfer });
    expect(head("main")).toHaveAttribute("data-drop", "above");
    fireEvent.drop(head("main"), { dataTransfer });
    expect(transport.callsOf("reorder_branches").at(-1)).toMatchObject({ keys: [FEAT_WT, MAIN_WT] });
    await waitFor(() => expect(rows()).toEqual(["feat/x", "main"]));
  });

  it("moves a worktree with Alt + arrow keys", async () => {
    const { store, user, transport } = setup();
    await open(store);
    rowButton("feat/x").focus();
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    expect(transport.callsOf("reorder_branches").at(-1)).toMatchObject({ keys: [FEAT_WT, MAIN_WT] });
    await waitFor(() => expect(rows()).toEqual(["feat/x", "main"]));
  });

  it("follows another client's reorder and a branch used elsewhere", async () => {
    const { store, transport } = setup();
    await open(store);
    expect(rows()).toEqual(["main", "feat/x"]);
    act(() => transport.emit(ev(0, { type: "branch_order_changed", project: PROJECT.id, keys: [FEAT_WT, MAIN_WT] }, true, "")));
    await waitFor(() => expect(rows()).toEqual(["feat/x", "main"]));
  });

  it("orders the other branches too, and reorders them from their menu", async () => {
    const { store, user, core, transport } = setup();
    core.git = { ...CHAT_GIT, branches: [...CHAT_GIT.branches, { name: "topic", sha: "t".repeat(40) }, { name: "alpha", sha: "a".repeat(40) }] };
    await open(store);
    await user.click(within(tree()).getByRole("button", { name: "他のブランチ (2)" }));
    const others = () => within(screen.getByRole("list", { name: "other branches" })).getAllByRole("button").map((b) => b.getAttribute("aria-label"));
    expect(others()).toEqual(["topic の新しいチャット", "alpha の新しいチャット"]);
    await user.pointer({ keys: "[MouseRight]", target: screen.getByRole("button", { name: "alpha の新しいチャット" }) });
    await user.click(within(screen.getByRole("menu", { name: "alpha の操作" })).getByRole("menuitem", { name: "上へ移動" }));
    expect(transport.callsOf("reorder_branches").at(-1)).toMatchObject({ keys: ["alpha", "topic"] });
    await waitFor(() => expect(others()).toEqual(["alpha の新しいチャット", "topic の新しいチャット"]));
  });
});
