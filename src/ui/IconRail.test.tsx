// The left rail: the dashboard, the work and runs views and the settings, and
// the projects that are open as rows with a status dot (a click shows one, a
// right click opens its menu, rows are reordered by drag or Alt + arrows).

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ProjectInfo, SessionRecord, Snapshot } from "../api/generated";
import { CommandError } from "../api/transport";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { ev, ORCHESTRATOR, prompted, turnEnded } from "../test/events";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { IconRail, type View } from "./IconRail";

const BETA: ProjectInfo = { id: "beta", name: "beta", path: "/tmp/beta", open: true, running: false, awake: true };
const GAMMA: ProjectInfo = { id: "gamma", name: "gamma", path: "/tmp/gamma", open: false, running: false, awake: false };
const DELTA: ProjectInfo = { id: "delta", name: "delta", path: "/tmp/delta", open: true, running: false, awake: true };

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
const working: SessionRecord = { ...idle, turn_running: true };

function setup(opts: { others?: ProjectInfo[]; snapshot?: Snapshot; view?: View; noProjects?: boolean } = {}) {
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
  const onChange = vi.fn();
  const onOpenSettings = vi.fn();
  render(
    <StoreContext.Provider value={store}>
      <IconRail view={opts.view ?? "work"} onChange={onChange} settingsOpen={false} onOpenSettings={onOpenSettings} />
    </StoreContext.Provider>,
  );
  return { transport, core, store, onChange, onOpenSettings, user: userEvent.setup() };
}

async function openRepo(store: AppStore) {
  await act(() => store.openProject(PROJECT.path));
  await waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
}

const rail = () => screen.getByRole("navigation", { name: "views" });
const list = () => screen.getByRole("list", { name: "開いているプロジェクト" });
const rows = () => within(list()).getAllByRole("button");
const names = () => rows().map((r) => within(r).getByText(/^[a-z]+$/).textContent);
const row = (name: string) => within(list()).getByRole("button", { name: new RegExp(`^${name} に切り替え`) });
const dot = (id: string) => screen.getByTestId(`rail-dot-${id}`);
const stubTransfer = () => ({ setData: vi.fn(), effectAllowed: "", dropEffect: "" });

describe("the buttons", () => {
  it("puts the dashboard first, the projects after it, and the views and the settings at the bottom", async () => {
    const { store } = setup();
    await openRepo(store);
    const labels = within(rail()).getAllByRole("button").map((b) => b.getAttribute("aria-label"));
    expect(labels).toEqual(["ダッシュボード", "repo に切り替え (停止)", "beta に切り替え (待機中)", "コーディング", "履歴", "設定"]);
  });

  it("keeps the labels in the document (the stylesheet shows them when the rail is expanded)", () => {
    setup();
    for (const label of ["ダッシュボード", "コーディング", "履歴", "設定"]) {
      const button = within(rail()).getByRole("button", { name: label });
      expect(within(button).getByText(label)).toHaveClass("rail-label");
      expect(button).toHaveAttribute("title", label);
    }
    expect(within(screen.getByRole("region", { name: "プロジェクト" })).getByRole("heading", { name: "プロジェクト" })).toBeInTheDocument();
  });

  it("marks the view that is shown", () => {
    setup({ view: "dashboard" });
    expect(screen.getByRole("button", { name: "ダッシュボード" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "ダッシュボード" })).toHaveClass("active");
    expect(screen.getByRole("button", { name: "コーディング" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "履歴" })).toHaveAttribute("aria-pressed", "false");
  });

  it("switches to the view of the button, and opens the settings", async () => {
    const { user, onChange, onOpenSettings } = setup();
    await user.click(screen.getByRole("button", { name: "ダッシュボード" }));
    await user.click(screen.getByRole("button", { name: "履歴" }));
    await user.click(screen.getByRole("button", { name: "コーディング" }));
    expect(onChange.mock.calls.map((c) => c[0])).toEqual(["dashboard", "runs", "work"]);
    const settings = screen.getByRole("button", { name: "設定" });
    expect(settings).toHaveAttribute("aria-haspopup", "dialog");
    expect(settings).not.toHaveAttribute("aria-pressed");
    await user.click(settings);
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });
});

describe("the projects", () => {
  it("lists the projects that are open, and the one on screen, not those that are not open", async () => {
    const { store } = setup();
    await openRepo(store);
    expect(names()).toEqual(["repo", "beta"]);
    expect(screen.queryByRole("button", { name: /^gamma/ })).not.toBeInTheDocument();
  });

  it("lists a project the core has not (yet) called open once it is on screen", async () => {
    const { store } = setup({ others: [{ ...BETA, open: false }, GAMMA] });
    await act(() => store.openProject("/tmp/beta"));
    await waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
    expect(names()).toEqual(["repo", "beta"]);
  });

  it("says so while nothing is open", async () => {
    setup({ noProjects: true });
    expect(await screen.findByText("開いているプロジェクトはありません。")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "開いているプロジェクト" })).toBeInTheDocument();
    expect(within(list()).queryAllByRole("button")).toEqual([]);
  });

  it("marks the project on screen as current, and as active while the work view is shown", async () => {
    const { store } = setup({ view: "work" });
    await openRepo(store);
    expect(row("repo")).toHaveAttribute("aria-current", "true");
    expect(row("repo")).toHaveClass("current", "active");
    expect(row("beta")).not.toHaveAttribute("aria-current");
    expect(row("beta")).not.toHaveClass("current", "active");
  });

  it("does not give the amber marker to the project while another view is shown", async () => {
    const { store } = setup({ view: "dashboard" });
    await openRepo(store);
    expect(row("repo")).toHaveClass("current");
    expect(row("repo")).not.toHaveClass("active");
  });

  it("describes a row with the project's path and state in its tooltip", async () => {
    const { store } = setup();
    await openRepo(store);
    expect(row("beta")).toHaveAttribute("title", "/tmp/beta · 待機中");
    expect(row("repo")).toHaveAttribute("title", "/tmp/repo · 停止");
  });
});

describe("the status dots", () => {
  it("is lit for the project with work on, hollow for an awake one and for a sleeping one", async () => {
    const { store } = setup({ snapshot: { ...FULL_RUN.start, sessions: [working] }, others: [BETA, { ...DELTA, awake: false }] });
    await openRepo(store);
    expect(dot("repo")).toHaveClass("project-dot", "project-dot-running");
    expect(dot("repo")).toHaveAttribute("data-ring", "running");
    expect(dot("beta")).toHaveClass("project-dot", "project-dot-idle");
    expect(dot("beta")).not.toHaveClass("project-dot-running");
    expect(dot("beta")).toHaveAttribute("data-ring", "ready");
    expect(dot("delta")).toHaveClass("project-dot-idle");
    expect(dot("delta")).toHaveAttribute("data-ring", "stopped");
  });

  it("is hollow for the project on screen while its chats all sleep", async () => {
    const { store } = setup();
    await openRepo(store);
    expect(dot("repo")).toHaveClass("project-dot-idle");
    expect(dot("repo")).toHaveAttribute("data-ring", "stopped");
  });

  it("lights up a project that is already working in the background", async () => {
    const { store } = setup({ others: [{ ...BETA, running: true }, GAMMA] });
    await openRepo(store);
    expect(dot("beta")).toHaveClass("project-dot-running");
    expect(row("beta")).toHaveAttribute("aria-label", "beta に切り替え (実行中)");
  });

  it("lights up the dot of a background project while a chat of it works, and puts it out when it stops", async () => {
    const { store, core, transport } = setup();
    await openRepo(store);
    expect(dot("beta")).toHaveClass("project-dot-idle");

    core.others = [{ ...BETA, running: true }, GAMMA];
    transport.emit({ ...prompted(1, ORCHESTRATOR), project: "beta" });
    await waitFor(() => expect(dot("beta")).toHaveClass("project-dot-running"));

    core.others = [{ ...BETA, running: false }, GAMMA];
    transport.emit({ ...turnEnded(2, ORCHESTRATOR), project: "beta" });
    await waitFor(() => expect(dot("beta")).toHaveClass("project-dot-idle"));
  });

  it("keeps the dot of the project switched away from lit while it works", async () => {
    const { store, core, user } = setup({ snapshot: { ...FULL_RUN.start, sessions: [working] } });
    core.info = { ...PROJECT, running: true }; // what the core says of it too
    await openRepo(store);
    expect(dot("repo")).toHaveClass("project-dot-running");

    core.snapshot = FULL_RUN.start; // beta has nothing going on
    await user.click(row("beta"));
    await waitFor(() => expect(store.getState().project?.info.id).toBe("beta"));
    expect(dot("repo")).toHaveClass("project-dot-running");
    await waitFor(() => expect(store.getState().project?.phase).toBe("ready"));
    expect(dot("beta")).toHaveClass("project-dot-idle");
    expect(dot("repo")).toHaveClass("project-dot-running");
  });

  it("keeps a project that was put to sleep in the rail with a hollow dot", async () => {
    const { store, core, transport } = setup();
    await openRepo(store);
    core.others = [{ ...BETA, awake: false }, GAMMA];
    transport.emit(ev(1, { type: "session_stopped", session: ORCHESTRATOR, suspended: false }, false, "beta"));
    await waitFor(() => expect(dot("beta")).toHaveAttribute("data-ring", "stopped"));
    expect(names()).toEqual(["repo", "beta"]);
  });
});

describe("showing a project", () => {
  it("opens the project that is clicked and switches to the work view", async () => {
    const { store, user, transport, onChange } = setup({ view: "dashboard" });
    await openRepo(store);
    await user.click(row("beta"));
    expect(transport.callsOf("open_project").at(-1)?.path).toBe("/tmp/beta");
    expect(onChange).toHaveBeenCalledWith("work");
    await waitFor(() => expect(row("beta")).toHaveAttribute("aria-current", "true"));
    expect(row("repo")).not.toHaveAttribute("aria-current");
  });

  it("puts the project it switches to first", async () => {
    const { store, user } = setup();
    await openRepo(store);
    await user.click(row("beta"));
    await waitFor(() => expect(names()).toEqual(["beta", "repo"]));
  });

  it("does not open the project that is on screen again, but still goes to the work view", async () => {
    const { store, user, transport, onChange } = setup({ view: "dashboard" });
    await openRepo(store);
    const before = transport.callsOf("open_project").length;
    await user.click(row("repo"));
    expect(transport.callsOf("open_project")).toHaveLength(before);
    expect(onChange).toHaveBeenCalledWith("work");
  });

  it("opens nothing while the core is not connected", async () => {
    const { store, user, transport, onChange } = setup();
    await openRepo(store);
    const before = transport.callsOf("open_project").length;
    act(() => transport.setStatus({ state: "closed", reason: "gone", retryInMs: 1000 }));
    expect(row("beta")).toHaveAttribute("aria-disabled", "true");
    await user.click(row("beta"));
    expect(transport.callsOf("open_project")).toHaveLength(before);
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("project order", () => {
  it("moves a project up or down from its right-click menu, and offers neither past the ends", async () => {
    const { store, user, transport } = setup({ others: [BETA, DELTA, GAMMA] });
    await openRepo(store);
    expect(names()).toEqual(["repo", "beta", "delta"]);
    await user.pointer({ keys: "[MouseRight]", target: row("beta") });
    const menu = screen.getByRole("menu", { name: "beta の操作" });
    await user.click(within(menu).getByRole("menuitem", { name: "上へ移動" }));
    // gamma is not in the rail: it keeps its place.
    expect(transport.callsOf("reorder_projects").at(-1)?.ids).toEqual(["beta", "repo", "delta", "gamma"]);
    await waitFor(() => expect(names()).toEqual(["beta", "repo", "delta"]));
    expect(row("beta")).toHaveFocus();

    await user.pointer({ keys: "[MouseRight]", target: row("beta") });
    expect(within(screen.getByRole("menu")).getByRole("menuitem", { name: "上へ移動" })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    await user.pointer({ keys: "[MouseRight]", target: row("delta") });
    expect(within(screen.getByRole("menu")).getByRole("menuitem", { name: "下へ移動" })).toBeDisabled();
  });

  it("trades places among the whole list when the rows shown are only some of the projects", async () => {
    const { store, user, transport } = setup({ others: [BETA, GAMMA, DELTA] });
    await openRepo(store);
    await user.pointer({ keys: "[MouseRight]", target: row("beta") });
    await user.click(within(screen.getByRole("menu")).getByRole("menuitem", { name: "下へ移動" }));
    // gamma (between beta and delta, not a row) stays where it is.
    expect(transport.callsOf("reorder_projects").at(-1)?.ids).toEqual(["repo", "delta", "gamma", "beta"]);
    await waitFor(() => expect(names()).toEqual(["repo", "delta", "beta"]));
  });

  it("moves the focused row with Alt + arrow keys", async () => {
    const { store, user, transport } = setup({ others: [BETA, DELTA, GAMMA] });
    await openRepo(store);
    row("beta").focus();
    await user.keyboard("{Alt>}{ArrowDown}{/Alt}");
    expect(transport.callsOf("reorder_projects").at(-1)?.ids).toEqual(["repo", "delta", "beta", "gamma"]);
    await waitFor(() => expect(names()).toEqual(["repo", "delta", "beta"]));
    // Nothing was opened or switched by moving.
    expect(transport.callsOf("open_project")).toHaveLength(1);
    row("beta").focus();
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    expect(transport.callsOf("reorder_projects").at(-1)?.ids).toEqual(["repo", "beta", "delta", "gamma"]);
  });

  it("reorders by dragging a project onto another", async () => {
    const { store, transport } = setup({ others: [BETA, DELTA, GAMMA] });
    await openRepo(store);
    const delta = row("delta").closest("li") as HTMLElement;
    const repo = row("repo").closest("li") as HTMLElement;
    expect(delta).toHaveAttribute("draggable", "true");
    const dataTransfer = stubTransfer();
    fireEvent.dragStart(delta, { dataTransfer });
    fireEvent.dragOver(repo, { dataTransfer });
    expect(repo).toHaveAttribute("data-drop", "above");
    fireEvent.drop(repo, { dataTransfer });
    expect(transport.callsOf("reorder_projects").at(-1)?.ids).toEqual(["delta", "repo", "beta", "gamma"]);
    await waitFor(() => expect(names()).toEqual(["delta", "repo", "beta"]));
    expect(row("repo").closest("li")).not.toHaveAttribute("data-drop");
  });

  it("does nothing when a project is dropped on itself", async () => {
    const { store, transport } = setup();
    await openRepo(store);
    const beta = row("beta").closest("li") as HTMLElement;
    const dataTransfer = stubTransfer();
    fireEvent.dragStart(beta, { dataTransfer });
    fireEvent.dragOver(beta, { dataTransfer });
    fireEvent.drop(beta, { dataTransfer });
    expect(transport.callsOf("reorder_projects")).toHaveLength(0);
  });
});

describe("sleeping a project", () => {
  const sleepItem = () => screen.getByRole("menuitem", { name: "スリープ" });

  it("opens a menu on a right click, with the moves and sleep, and sleeps the project without opening it", async () => {
    const { store, user, transport } = setup();
    await openRepo(store);
    await user.pointer({ keys: "[MouseRight]", target: row("beta") });
    const menu = screen.getByRole("menu", { name: "beta の操作" });
    expect(within(menu).getAllByRole("menuitem").map((i) => i.textContent)).toEqual(["上へ移動", "下へ移動", "スリープ"]);
    expect(sleepItem()).toBeEnabled();
    expect(transport.callsOf("open_project").map((c) => c.path)).toEqual(["/tmp/repo"]);
    await user.click(sleepItem());
    expect(transport.callsOf("sleep_project")).toEqual([{ type: "sleep_project", project: "beta" }]);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(transport.callsOf("open_project").map((c) => c.path)).toEqual(["/tmp/repo"]);
  });

  it("disables sleep for a project while a chat of it works", async () => {
    const { store, user, transport } = setup({ others: [{ ...BETA, running: true }, GAMMA] });
    await openRepo(store);
    await user.pointer({ keys: "[MouseRight]", target: row("beta") });
    expect(sleepItem()).toBeDisabled();
    await user.click(sleepItem());
    expect(transport.callsOf("sleep_project")).toEqual([]);
  });

  it("sleeps the project on screen when one of its chats is awake and idle", async () => {
    const { store, user, transport } = setup({ snapshot: { ...FULL_RUN.start, sessions: [idle] } });
    await openRepo(store);
    await user.pointer({ keys: "[MouseRight]", target: row("repo") });
    expect(screen.getByRole("menu", { name: "repo の操作" })).toBeInTheDocument();
    await user.click(sleepItem());
    expect(transport.callsOf("sleep_project")).toEqual([{ type: "sleep_project", project: "repo" }]);
  });

  it("disables sleep for the project on screen while its chats all sleep, or while one works", async () => {
    const { store, user } = setup();
    await openRepo(store);
    await user.pointer({ keys: "[MouseRight]", target: row("repo") });
    expect(sleepItem()).toBeDisabled();
  });

  it("opens the menu of the focused row from the menu key", async () => {
    const { store } = setup();
    await openRepo(store);
    fireEvent.contextMenu(row("beta"));
    expect(screen.getByRole("menu", { name: "beta の操作" })).toBeInTheDocument();
  });

  it("closes the menu with Esc and gives the focus back to the row", async () => {
    const { store, user } = setup();
    await openRepo(store);
    await user.pointer({ keys: "[MouseRight]", target: row("beta") });
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(row("beta")).toHaveFocus();
  });

  it("shows the core's refusal as an error", async () => {
    const { store, core } = setup();
    await openRepo(store);
    core.failures.set("sleep_project", new CommandError("invalid_state", "the orchestrator of chat C-1 is working"));
    await act(() => store.sleepProject("beta"));
    expect(store.getState().errors.map((e) => e.message)).toEqual(["sleep_project failed: the orchestrator of chat C-1 is working"]);
  });
});
