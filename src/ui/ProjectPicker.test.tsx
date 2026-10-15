// The project pull-down and the projects running in the background under it
// (Sidebar).

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import type { ProjectInfo, SessionRecord, Snapshot } from "../api/generated";
import { CommandError } from "../api/transport";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { ev, prompted, turnEnded, ORCHESTRATOR } from "../test/events";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { Sidebar } from "./Sidebar";

const BETA: ProjectInfo = { id: "beta", name: "beta", path: "/tmp/beta", open: true, running: false, awake: true };
const GAMMA: ProjectInfo = { id: "gamma", name: "gamma", path: "/tmp/gamma", open: false, running: false, awake: false };

function setup(opts: { others?: ProjectInfo[]; snapshot?: Snapshot; noProjects?: boolean } = {}) {
  const transport = new MemoryTransport();
  const core = new FakeCore(PROJECT, opts.snapshot ?? FULL_RUN.start);
  core.others = opts.others ?? [BETA, GAMMA];
  core.attach(transport);
  if (opts.noProjects) {
    // The core knows no project yet (set before the store asks for the list).
    const serve = transport.handler;
    transport.handler = (cmd) => (cmd.type === "list_projects" ? { type: "projects", projects: [] } : serve(cmd));
  }
  const store = new AppStore(transport);
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <Sidebar />
    </StoreContext.Provider>,
  );
  return { transport, core, store, user: userEvent.setup() };
}

async function openRepo(store: AppStore) {
  await act(() => store.openProject(PROJECT.path));
  await waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
}

const trigger = () => screen.getByRole("button", { name: "repo" });

describe("project pull-down", () => {
  it("shows only the open project until it is opened, then lists every known project", async () => {
    const { store, user } = setup();
    await openRepo(store);
    expect(trigger()).toHaveAttribute("aria-haspopup", "listbox");
    expect(trigger()).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.queryByText("gamma")).not.toBeInTheDocument();

    await user.click(trigger());
    expect(trigger()).toHaveAttribute("aria-expanded", "true");
    const list = screen.getByRole("listbox", { name: "プロジェクト" });
    const options = within(list).getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual(["repo", "beta", "gamma"]);
    expect(options.map((o) => o.getAttribute("aria-selected"))).toEqual(["true", "false", "false"]);
    expect(list).toHaveAttribute("aria-activedescendant", options[0].id);
    expect(list).toHaveFocus();
  });

  it("switches project when an option is clicked and closes", async () => {
    const { store, user, transport } = setup();
    await openRepo(store);
    await user.click(trigger());
    await user.click(screen.getByRole("option", { name: "gamma" }));
    expect(transport.callsOf("open_project").at(-1)?.path).toBe("/tmp/gamma");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "gamma" })).toHaveAttribute("aria-expanded", "false");
  });

  it("choosing the project already shown just closes", async () => {
    const { store, user, transport } = setup();
    await openRepo(store);
    const before = transport.callsOf("open_project").length;
    await user.click(trigger());
    await user.click(screen.getByRole("option", { name: "repo" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(transport.callsOf("open_project")).toHaveLength(before);
  });

  it("is driven from the keyboard: arrows open and move, Enter picks, Esc closes", async () => {
    const { store, user, transport } = setup();
    await openRepo(store);
    trigger().focus();
    await user.keyboard("{ArrowDown}");
    const list = screen.getByRole("listbox");
    expect(list).toHaveFocus();
    const options = within(list).getAllByRole("option");
    await user.keyboard("{ArrowDown}");
    expect(list).toHaveAttribute("aria-activedescendant", options[1].id);
    await user.keyboard("{End}");
    expect(list).toHaveAttribute("aria-activedescendant", options[2].id);
    await user.keyboard("{ArrowDown}");
    expect(list).toHaveAttribute("aria-activedescendant", options[2].id);
    await user.keyboard("{Home}{ArrowUp}");
    expect(list).toHaveAttribute("aria-activedescendant", options[0].id);

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(trigger()).toHaveFocus();
    expect(transport.callsOf("open_project")).toHaveLength(1);

    await user.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(transport.callsOf("open_project").at(-1)?.path).toBe("/tmp/beta");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "beta" })).toHaveFocus();
  });

  it("closes when the focus leaves it or the trigger is clicked again", async () => {
    const { store, user } = setup();
    await openRepo(store);
    await user.click(trigger());
    await user.click(document.body);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    await user.click(trigger());
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    await user.click(trigger());
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("opens another repository from the form in the pull-down", async () => {
    const { store, user, transport } = setup();
    await openRepo(store);
    await user.click(trigger());
    const open = screen.getByRole("button", { name: "開く" });
    expect(open).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "開くリポジトリのパス" }), "  /tmp/new  ");
    await user.click(open);
    expect(transport.callsOf("open_project").at(-1)?.path).toBe("/tmp/new");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("offers the form alone while no project is known, and names what to do on the trigger", async () => {
    const { user } = setup({ noProjects: true });
    await user.click(screen.getByRole("button", { name: "プロジェクトを開く" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.getByText("まだプロジェクトがありません。")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "開くリポジトリのパス" })).toHaveFocus();
  });

  it("shows the state of the open project on the trigger", async () => {
    const running: SessionRecord = {
      session_key: "orchestrator:C-1",
      role: "orchestrator",
      task: null,
      acp_session_id: "acp",
      status: "live",
      turn_running: true,
      agent: null,
      cwd: null,
    };
    const { store } = setup({ snapshot: { ...FULL_RUN.start, sessions: [running] } });
    await openRepo(store);
    expect(screen.getByTestId("current-ring")).toHaveClass("ring-running");
  });
});

const idle: SessionRecord = {
  session_key: "orchestrator:C-1",
  role: "orchestrator",
  task: null,
  acp_session_id: "acp",
  status: "live",
  turn_running: false,
  agent: null,
  cwd: null,
};

describe("project status rings", () => {
  const running: SessionRecord = {
    session_key: "orchestrator:C-1",
    role: "orchestrator",
    task: null,
    acp_session_id: "acp",
    status: "live",
    turn_running: true,
    agent: null,
    cwd: null,
  };

  it("shows a stopped ring on the trigger while every chat of the project sleeps", async () => {
    const { store } = setup();
    await openRepo(store);
    expect(screen.getByTestId("current-ring")).toHaveClass("ring", "ring-stopped");
    expect(screen.getByTestId("current-ring")).not.toHaveClass("ring-running");
  });

  it("shows a hollow green ring on the trigger while a chat is awake but idle", async () => {
    const idle: SessionRecord = {
      session_key: "orchestrator:C-1",
      role: "orchestrator",
      task: null,
      acp_session_id: "acp",
      status: "live",
      turn_running: false,
      agent: null,
      cwd: null,
    };
    const { store } = setup({ snapshot: { ...FULL_RUN.start, sessions: [idle] } });
    await openRepo(store);
    expect(screen.getByTestId("current-ring")).toHaveClass("ring", "ring-ready");
    expect(screen.getByTestId("current-ring")).not.toHaveClass("ring-running");
  });

  it("gives each option a ring: turning for the project with work on, hollow green for an awake one, gray for a sleeping one", async () => {
    const { store, user } = setup({ snapshot: { ...FULL_RUN.start, sessions: [running] } });
    await openRepo(store);
    await user.click(trigger());
    expect(screen.getByTestId("ring-repo")).toHaveClass("ring", "ring-running");
    expect(screen.getByTestId("ring-beta")).toHaveClass("ring", "ring-ready");
    expect(screen.getByTestId("ring-beta")).not.toHaveClass("ring-running");
    expect(screen.getByTestId("ring-gamma")).toHaveClass("ring", "ring-stopped");
    expect(screen.getByTestId("ring-gamma")).not.toHaveClass("ring-ready");
    expect(screen.getByRole("option", { name: "gamma" })).toHaveAttribute("title", "/tmp/gamma · 停止");
  });

  it("gives an idle background project the hollow ring instead of a lit dot, even while the shown one works", async () => {
    const { store } = setup({ snapshot: { ...FULL_RUN.start, sessions: [running] } });
    await openRepo(store);
    const ring = screen.getByTestId("bg-ring-beta");
    expect(ring).toHaveClass("ring", "ring-ready");
    expect(ring).not.toHaveClass("ring-running");
    expect(ring.parentElement).toHaveAttribute("aria-label", "beta に切り替え (待機中)");
    expect(document.querySelector(".bg-dot")).toBeNull();
  });

  it("turns the ring of a background project while a chat of it works, and hollows it when it stops", async () => {
    const { store, core, transport, user } = setup();
    await openRepo(store);
    expect(screen.getByTestId("bg-ring-beta")).toHaveClass("ring-ready");

    core.others = [{ ...BETA, running: true }, GAMMA];
    transport.emit({ ...prompted(1, ORCHESTRATOR), project: "beta" });
    await waitFor(() => expect(screen.getByTestId("bg-ring-beta")).toHaveClass("ring", "ring-running"));
    expect(screen.getByTestId("bg-ring-beta")).not.toHaveClass("ring-ready");
    expect(screen.getByRole("button", { name: "beta に切り替え (実行中)" })).toBeInTheDocument();
    await user.click(trigger());
    expect(screen.getByTestId("ring-beta")).toHaveClass("ring-running");
    expect(screen.getByTestId("ring-repo")).toHaveClass("ring-stopped");

    core.others = [{ ...BETA, running: false }, GAMMA];
    transport.emit({ ...turnEnded(2, ORCHESTRATOR), project: "beta" });
    await waitFor(() => expect(screen.getByTestId("bg-ring-beta")).toHaveClass("ring", "ring-ready"));
    expect(screen.getByTestId("bg-ring-beta")).not.toHaveClass("ring-running");
    expect(screen.getByTestId("ring-beta")).toHaveClass("ring-ready");
  });

  it("shows a background project that was already working as turning from the start", async () => {
    const { store } = setup({ others: [{ ...BETA, running: true }, GAMMA] });
    await openRepo(store);
    expect(screen.getByTestId("bg-ring-beta")).toHaveClass("ring", "ring-running");
  });

  it("keeps turning the ring of the project switched away from while it works", async () => {
    const { store, core, user } = setup({ snapshot: { ...FULL_RUN.start, sessions: [running] } });
    core.info = { ...PROJECT, running: true }; // what the core says of it too
    await openRepo(store);
    expect(screen.getByTestId("current-ring")).toHaveClass("ring-running");

    core.snapshot = FULL_RUN.start; // beta has nothing going on
    await user.click(screen.getByRole("button", { name: /beta に切り替え/ }));
    await waitFor(() => expect(store.getState().project?.info.id).toBe("beta"));
    expect(screen.getByTestId("bg-ring-repo")).toHaveClass("ring", "ring-running");
    await waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
    expect(screen.getByTestId("current-ring")).toHaveClass("ring", "ring-stopped");
    expect(screen.getByTestId("bg-ring-repo")).toHaveClass("ring-running");
  });
});

describe("projects running in the background", () => {
  it("lists the open projects that are not on screen, each with an indicator", async () => {
    const { store } = setup();
    await openRepo(store);
    const list = await screen.findByRole("list", { name: "バックグラウンドで動作中のプロジェクト" });
    const chips = within(list).getAllByRole("button");
    expect(chips.map((c) => c.textContent)).toEqual(["beta"]);
    expect(screen.getByTestId("bg-ring-beta")).toBeInTheDocument();
    expect(within(list).queryByText("gamma")).not.toBeInTheDocument();
    expect(within(list).queryByText("repo")).not.toBeInTheDocument();
  });

  it("switches to the project on click, and the one left takes its place", async () => {
    // repo has an orchestrator up when it is left, so it is still running in the background.
    const { store, user, transport } = setup({ snapshot: { ...FULL_RUN.start, sessions: [idle] } });
    await openRepo(store);
    await user.click(await screen.findByRole("button", { name: /beta に切り替え/ }));
    expect(transport.callsOf("open_project").at(-1)?.path).toBe("/tmp/beta");
    const list = await screen.findByRole("list", { name: "バックグラウンドで動作中のプロジェクト" });
    await waitFor(() => expect(within(list).getAllByRole("button").map((c) => c.textContent)).toEqual(["repo"]));
    expect(screen.getByRole("button", { name: "beta" })).toBeInTheDocument();
  });

  it("is not shown when no other project is open", async () => {
    const { store, user } = setup({ others: [GAMMA] });
    await openRepo(store);
    await user.click(screen.getByRole("button", { name: "repo" }));
    expect(screen.getByRole("option", { name: "gamma" })).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "バックグラウンドで動作中のプロジェクト" })).not.toBeInTheDocument();
  });
});

describe("projects running in the background: a project that sleeps leaves", () => {
  it("is not listed while every chat of the open project sleeps, though it stays in the pull-down", async () => {
    const { store, user } = setup({ others: [{ ...BETA, awake: false }, GAMMA] });
    await openRepo(store);
    expect(screen.queryByRole("list", { name: "バックグラウンドで動作中のプロジェクト" })).not.toBeInTheDocument();
    await user.click(trigger());
    expect(screen.getByTestId("ring-beta")).toHaveClass("ring-stopped");
  });

  it("vanishes from the chips once the core reports it asleep", async () => {
    const { store, core, transport } = setup();
    await openRepo(store);
    expect(screen.getByTestId("bg-ring-beta")).toBeInTheDocument();
    core.others = [{ ...BETA, awake: false }, GAMMA];
    transport.emit(ev(1, { type: "session_stopped", session: ORCHESTRATOR, suspended: false }, false, "beta"));
    await waitFor(() => expect(screen.queryByTestId("bg-ring-beta")).not.toBeInTheDocument());
    expect(screen.queryByRole("list", { name: "バックグラウンドで動作中のプロジェクト" })).not.toBeInTheDocument();
  });
});

describe("sleeping a project", () => {
  const sleepItem = () => screen.getByRole("menuitem", { name: "スリープ" });

  it("opens a menu with sleep on a right click on a background chip, and sleeps the project", async () => {
    const { store, user, transport } = setup();
    await openRepo(store);
    await user.pointer({ keys: "[MouseRight]", target: screen.getByRole("button", { name: /beta に切り替え/ }) });
    const menu = screen.getByRole("menu", { name: "beta の操作" });
    expect(within(menu).getAllByRole("menuitem").map((i) => i.textContent)).toEqual(["スリープ"]);
    expect(sleepItem()).toBeEnabled();
    expect(transport.callsOf("open_project").map((c) => c.path)).toEqual(["/tmp/repo"]);
    await user.click(sleepItem());
    expect(transport.callsOf("sleep_project")).toEqual([{ type: "sleep_project", project: "beta" }]);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(transport.callsOf("open_project").map((c) => c.path)).toEqual(["/tmp/repo"]);
  });

  it("disables sleep for a background project while a chat of it works", async () => {
    const { store, user, transport } = setup({ others: [{ ...BETA, running: true }, GAMMA] });
    await openRepo(store);
    await user.pointer({ keys: "[MouseRight]", target: screen.getByRole("button", { name: /beta に切り替え/ }) });
    expect(sleepItem()).toBeDisabled();
    await user.click(sleepItem());
    expect(transport.callsOf("sleep_project")).toEqual([]);
  });

  it("opens the menu on a right click on a row of the pull-down, and closes the pull-down after sleeping", async () => {
    const { store, user, transport } = setup();
    await openRepo(store);
    await user.click(trigger());
    await user.pointer({ keys: "[MouseRight]", target: screen.getByRole("option", { name: "beta" }) });
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    expect(sleepItem()).toBeEnabled();
    expect(transport.callsOf("open_project").map((c) => c.path)).toEqual(["/tmp/repo"]);
    await user.click(sleepItem());
    expect(transport.callsOf("sleep_project")).toEqual([{ type: "sleep_project", project: "beta" }]);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(transport.callsOf("open_project").map((c) => c.path)).toEqual(["/tmp/repo"]);
  });

  it("disables sleep for a project of the pull-down that is not open", async () => {
    const { store, user, transport } = setup();
    await openRepo(store);
    await user.click(trigger());
    await user.pointer({ keys: "[MouseRight]", target: screen.getByRole("option", { name: "gamma" }) });
    expect(screen.getByRole("menu", { name: "gamma の操作" })).toBeInTheDocument();
    expect(sleepItem()).toBeDisabled();
    expect(transport.callsOf("sleep_project")).toEqual([]);
  });

  it("Esc in the menu closes only the menu, the pull-down stays", async () => {
    const { store, user } = setup();
    await openRepo(store);
    await user.click(trigger());
    await user.pointer({ keys: "[MouseRight]", target: screen.getByRole("option", { name: "beta" }) });
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    expect(screen.getByRole("listbox")).toHaveFocus();
  });

  it("sleeps the project on screen from a right click on the pull-down button", async () => {
    const { store, user, transport } = setup({ snapshot: { ...FULL_RUN.start, sessions: [idle] } });
    await openRepo(store);
    await user.pointer({ keys: "[MouseRight]", target: trigger() });
    expect(screen.getByRole("menu", { name: "repo の操作" })).toBeInTheDocument();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    await user.click(sleepItem());
    expect(transport.callsOf("sleep_project")).toEqual([{ type: "sleep_project", project: "repo" }]);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("disables sleep for the project on screen while its chats all sleep", async () => {
    const { store, user } = setup();
    await openRepo(store);
    await user.pointer({ keys: "[MouseRight]", target: trigger() });
    expect(sleepItem()).toBeDisabled();
  });

  it("opens the menu of the active row when the focused list is asked for one (the menu key)", async () => {
    const { store, user } = setup();
    await openRepo(store);
    await user.click(trigger());
    await user.keyboard("{ArrowDown}");
    fireEvent.contextMenu(screen.getByRole("listbox"));
    expect(screen.getByRole("menu", { name: "beta の操作" })).toBeInTheDocument();
    expect(sleepItem()).toBeEnabled();
  });

  it("disables sleep for the project on screen while a chat of it works", async () => {
    const working: SessionRecord = { ...idle, turn_running: true };
    const { store, user } = setup({ snapshot: { ...FULL_RUN.start, sessions: [working] } });
    await openRepo(store);
    await user.pointer({ keys: "[MouseRight]", target: trigger() });
    expect(sleepItem()).toBeDisabled();
  });

  it("shows the core's refusal as an error", async () => {
    const { store, core } = setup();
    await openRepo(store);
    core.failures.set("sleep_project", new CommandError("invalid_state", "the orchestrator of chat C-1 is working"));
    await act(() => store.sleepProject("beta"));
    expect(store.getState().errors.map((e) => e.message)).toEqual([
      "sleep_project failed: the orchestrator of chat C-1 is working",
    ]);
  });
});
