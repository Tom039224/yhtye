// The project pull-down and the projects running in the background under it
// (Sidebar).

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import type { ProjectInfo, SessionRecord, Snapshot } from "../api/generated";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { Sidebar } from "./Sidebar";

const BETA: ProjectInfo = { id: "beta", name: "beta", path: "/tmp/beta", open: true };
const GAMMA: ProjectInfo = { id: "gamma", name: "gamma", path: "/tmp/gamma", open: false };

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
      session_key: "orchestrator/C-1",
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
    const { store, user, transport } = setup();
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
