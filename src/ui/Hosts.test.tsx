// The screens of the hosts (T-97, T-98): the project pull-down across hosts with
// the mark of a remote one, the banner and the footer of a remote host on
// screen. The registry and the stores are real; the shell and the remote hosts
// are fakes (src/test/remoteHosts.ts).

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { RemoteDevice } from "../api/generated";
import { LOCAL_HOST } from "../store/hosts";
import { HostsProvider } from "../store/useHosts";
import { FAR_ONLY, FAR_REPO, FakeHostsApi, hostInfo, newTestHosts, type TestHosts } from "../test/remoteHosts";
import { ConnectionBanner } from "./ConnectionBanner";
import { Sidebar } from "./Sidebar";

let t: TestHosts;

function mount(ui: React.ReactElement) {
  render(<HostsProvider registry={t.registry}>{ui}</HostsProvider>);
  return userEvent.setup();
}

async function started(hosts: string[] = ["sid-1"]) {
  const api = t.api as FakeHostsApi;
  api.hosts = hosts.map((id) => hostInfo(id));
  await act(() => t.registry.start());
  await waitFor(() => expect(t.registry.getState().projects.length).toBeGreaterThanOrEqual(2 + hosts.length * 2));
}

/** Shows the remote project of `host`, as a pick in the pull-down does. */
async function showRemote(host = "sid-1") {
  await act(() => t.registry.selectProject(host, FAR_ONLY));
  await waitFor(() => expect(t.registry.getState().active).toBe(host));
}

const marks = (within_: HTMLElement) => within(within_).queryAllByRole("img", { name: /^リモート/ });

beforeEach(() => {
  t = newTestHosts();
});

afterEach(() => {
  t.registry.stop();
});

describe("the project pull-down across hosts", () => {
  it("lists this PC's projects first, then each remote host's, a remote one marked with its host", async () => {
    const user = mount(<Sidebar />);
    await started(["sid-1", "sid-2"]);
    await user.click(screen.getByRole("button", { name: "プロジェクトを選択" }));
    const options = within(screen.getByRole("listbox", { name: "プロジェクト" })).getAllByRole("option");
    // `repo` is a project of three hosts and `server-only` of two: those rows say whose they are.
    expect(options.map((o) => o.textContent)).toEqual([
      "repo",
      "beta",
      "repohost-sid-1",
      "server-onlyhost-sid-1",
      "repohost-sid-2",
      "server-onlyhost-sid-2",
    ]);
    expect(options.map((o) => marks(o).length)).toEqual([0, 0, 1, 1, 1, 1]);
    expect(options[2]).toHaveAttribute("title", expect.stringContaining("/srv/repo"));
  });

  it("marks a remote project with the host's name and state on hover, and says nothing more where the name is its own", async () => {
    const user = mount(<Sidebar />);
    await started();
    await user.click(screen.getByRole("button", { name: "プロジェクトを選択" }));
    const options = screen.getAllByRole("option");
    // `server-only` is only on the host: no host name beside it, but the mark has it.
    expect(options[3].textContent).toBe("server-only");
    const mark = marks(options[3])[0];
    expect(mark).toHaveAttribute("title", "host-sid-1 · 接続中");
    expect(mark).not.toHaveClass("down");
    expect(mark.querySelector("svg")).toHaveAttribute("data-icon", "remote");
  });

  it("greys the mark of a host that is not connected, and does not pick its project", async () => {
    const user = mount(<Sidebar />);
    await started();
    (t.api as FakeHostsApi).notice({
      type: "status",
      server_id: "sid-1",
      status: { connection: { state: "waiting_to_reconnect", attempt: 1, delay_ms: 1000, reason: "refused" }, server: null },
    });
    await user.click(screen.getByRole("button", { name: "プロジェクトを選択" }));
    const option = screen.getByRole("option", { name: /server-only/ });
    expect(option).toHaveAttribute("aria-disabled", "true");
    const mark = marks(option)[0];
    expect(mark).toHaveClass("down");
    expect(mark).toHaveAttribute("title", "host-sid-1 · 切断 (再接続待ち)");
    await user.click(option);
    expect(t.registry.getState().active).toBe(LOCAL_HOST);
    expect(t.fars.get("sid-1")?.transport.callsOf("open_project")).toEqual([]);
  });

  it("looks as it did when no host is paired with: no mark, no host name", async () => {
    const user = mount(<Sidebar />);
    await started([]);
    await user.click(screen.getByRole("button", { name: "プロジェクトを選択" }));
    const options = screen.getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual(["repo", "beta"]);
    expect(screen.queryAllByRole("img", { name: /^リモート/ })).toEqual([]);
    expect(document.querySelector(".host-tag, .remote-mark")).toBeNull();
  });

  it("switches the screen to the host of the project that is picked, and opens it there", async () => {
    const user = mount(<Sidebar />);
    await started();
    await user.click(screen.getByRole("button", { name: "プロジェクトを選択" }));
    await user.click(screen.getByRole("option", { name: /server-only/ }));
    expect(t.registry.getState().active).toBe("sid-1");
    await waitFor(() => expect(screen.getByRole("button", { name: /^server-only/ })).toBeInTheDocument());
    expect(t.fars.get("sid-1")?.transport.callsOf("open_project").map((c) => c.path)).toEqual([FAR_ONLY.path]);
    expect(t.localTransport.callsOf("open_project")).toEqual([]);

    // The list is still every host's, the open one is marked, and so is the pull-down itself.
    // Opening a project uses it (T-103), so within its host's rows it is the first.
    const trigger = screen.getByRole("button", { name: /^server-only/ });
    expect(marks(trigger)).toHaveLength(1);
    await user.click(trigger);
    const options = screen.getAllByRole("option");
    expect(options.map((o) => o.getAttribute("aria-selected"))).toEqual(["false", "false", "true", "false"]);
  });

  it("keeps the same project of another host apart from the one on screen", async () => {
    const user = mount(<Sidebar />);
    await started();
    await act(() => t.registry.selectProject(LOCAL_HOST, t.registry.getState().projects[0].project));
    await waitFor(() => expect(t.localTransport.callsOf("open_project")).toHaveLength(1));
    await user.click(screen.getByRole("button", { name: "repo" }));
    // Both rows are called repo; only this app's is the open one.
    const options = screen.getAllByRole("option", { name: /^repo/ });
    expect(options.map((o) => o.getAttribute("aria-selected"))).toEqual(["true", "false"]);
    // The trigger of a local project has no mark.
    expect(marks(screen.getByRole("button", { name: "repo" }))).toEqual([]);
  });

  it("opens a repository on the host that is on screen, and says whose path it takes", async () => {
    const user = mount(<Sidebar />);
    await started();
    await user.click(screen.getByRole("button", { name: "プロジェクトを選択" }));
    expect(screen.getByRole("textbox", { name: "開くリポジトリのパス" })).toHaveAttribute("placeholder", "/path/to/git/repository");
    await user.keyboard("{Escape}");

    await showRemote();
    await user.click(screen.getByRole("button", { name: /^server-only/ }));
    const input = screen.getByRole("textbox", { name: "開くリポジトリのパス (host-sid-1 上)" });
    expect(input).toHaveAttribute("placeholder", "host-sid-1 上のパス");
    await user.type(input, "/srv/other{Enter}");
    expect(t.fars.get("sid-1")?.transport.callsOf("open_project").map((c) => c.path)).toContain("/srv/other");
    expect(t.localTransport.callsOf("open_project").map((c) => c.path)).not.toContain("/srv/other");
  });
});

describe("sleeping and moving projects in the pull-down across hosts", () => {
  it("offers the menu and the drag on the rows of this PC only while this PC is on screen", async () => {
    const user = mount(<Sidebar />);
    await started();
    await user.click(screen.getByRole("button", { name: "プロジェクトを選択" }));
    const options = screen.getAllByRole("option");
    // repo, beta (this PC's), then repo, server-only (the host's).
    expect(options.map((o) => o.getAttribute("draggable"))).toEqual(["true", "true", null, null]);

    await user.pointer({ keys: "[MouseRight]", target: options[3] });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    await user.pointer({ keys: "[MouseRight]", target: options[1] });
    const menu = screen.getByRole("menu", { name: "beta の操作" });
    await user.click(within(menu).getByRole("menuitem", { name: "上へ移動" }));
    expect(t.localTransport.callsOf("reorder_projects").at(-1)?.ids).toEqual(["beta", "repo"]);
    expect(t.fars.get("sid-1")?.transport.callsOf("reorder_projects")).toEqual([]);
  });

  it("sends the move of a remote host's project to that host, and leaves this PC's rows alone", async () => {
    const user = mount(<Sidebar />);
    await started();
    await showRemote();
    await user.click(screen.getByRole("button", { name: /^server-only/ }));
    const options = screen.getAllByRole("option");
    // The host's own rows are the ones that can be moved now.
    expect(options.map((o) => o.getAttribute("draggable"))).toEqual([null, null, "true", "true"]);

    await user.pointer({ keys: "[MouseRight]", target: options[0] });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    // Opening server-only used it: it is the host's first.
    await user.pointer({ keys: "[MouseRight]", target: options[2] });
    const menu = screen.getByRole("menu", { name: "server-only の操作" });
    expect(within(menu).getByRole("menuitem", { name: "上へ移動" })).toBeDisabled();
    await user.click(within(menu).getByRole("menuitem", { name: "下へ移動" }));
    expect(t.fars.get("sid-1")?.transport.callsOf("reorder_projects").at(-1)?.ids).toEqual(["repo", "server-only"]);
    expect(t.localTransport.callsOf("reorder_projects")).toEqual([]);
  });
});

describe("the projects running in the background", () => {
  it("carry the mark when they are a remote host's, and not otherwise", async () => {
    mount(<Sidebar />);
    await started();
    // This PC's own `beta` is open in the background.
    expect(await screen.findByRole("button", { name: /^beta に切り替え/ })).toBeInTheDocument();
    expect(screen.queryAllByRole("img", { name: /^リモート/ })).toEqual([]);

    const far = t.fars.get("sid-1")!;
    far.core.others = [{ ...FAR_ONLY, open: true, awake: true }];
    await act(() => far.store.refreshProjects());
    await act(() => t.registry.selectProject("sid-1", FAR_REPO));
    const chip = await screen.findByRole("button", { name: /^server-only に切り替え/ });
    expect(marks(chip)).toHaveLength(1);
    expect(screen.queryByRole("button", { name: /^beta に切り替え/ })).not.toBeInTheDocument();
  });
});

describe("the banner of a remote host on screen", () => {
  it("names the host when its connection is lost, and not when this PC's core is", async () => {
    mount(<ConnectionBanner />);
    await started();
    expect(screen.queryByTestId("connection-lost")).not.toBeInTheDocument();
    await showRemote();
    (t.api as FakeHostsApi).notice({
      type: "status",
      server_id: "sid-1",
      status: { connection: { state: "waiting_to_reconnect", attempt: 1, delay_ms: 2000, reason: "connection refused" }, server: null },
    });
    expect(await screen.findByTestId("connection-lost")).toHaveTextContent("host-sid-1 との接続が切れました: connection refused · 2 秒後に再接続");
  });

  it("says a host that removed this device did so", async () => {
    mount(<ConnectionBanner />);
    await started();
    await showRemote();
    (t.api as FakeHostsApi).notice({ type: "status", server_id: "sid-1", status: { connection: { state: "revoked" }, server: null } });
    expect(await screen.findByTestId("connection-lost")).toHaveTextContent("host-sid-1 との接続が切れました: この端末は接続先から外されました");
  });

  it("warns, with the host's name, when its version does not fit this app's", async () => {
    mount(<ConnectionBanner />);
    await started();
    await showRemote();
    expect(screen.queryByTestId("host-incompatible")).not.toBeInTheDocument();
    const info = hostInfo("sid-1");
    (t.api as FakeHostsApi).notice({
      type: "status",
      server_id: "sid-1",
      status: { ...info.status, server: { ...info.status.server!, compatibility: { type: "incompatible", host: "0.2.0", app: "0.1.0" } } },
    });
    const warning = await screen.findByTestId("host-incompatible");
    expect(warning).toHaveTextContent("host-sid-1 の Yhtye: バージョンが合いません (接続先 0.2.0 / この PC のアプリ 0.1.0)");
  });
});

describe("the footer of a remote host on screen", () => {
  it("shows that host: its name with the mark, its own kind of icon, and the devices paired with it", async () => {
    const device = (id: string, name: string, kind: RemoteDevice["kind"]): RemoteDevice => ({ id, name, kind, created_ms: 1, last_seen_ms: 2, connected: true });
    t.registry.stop();
    t = newTestHosts({
      configureFar: (far) => {
        far.core.headless = true;
        far.core.remote.devices = [device("d-1", "Pixel 8", "android"), device("d-2", "this pc", "desktop")];
      },
    });
    mount(<Sidebar />);
    await started();
    // This PC's own core first: no mark.
    const footer = () => screen.getByTestId("host");
    expect(within(footer()).getByText("test-host")).toBeInTheDocument();
    expect(marks(footer())).toEqual([]);

    await showRemote();
    await waitFor(() => expect(within(footer()).getByText("host-sid-1")).toBeInTheDocument());
    expect(footer().querySelector("[data-icon='server-headless']")).not.toBeNull();
    expect(marks(footer())).toHaveLength(1);
    expect(footer()).toHaveAttribute("title", expect.stringContaining("host-sid-1 · リモート · ヘッドレス"));
    const clients = await screen.findByRole("list", { name: "クライアント" });
    const icons = within(clients)
      .getAllByRole("listitem")
      .map((li) => li.querySelector("svg")?.getAttribute("data-icon"));
    expect(icons.sort()).toEqual(["monitor", "phone"]);
  });
});
