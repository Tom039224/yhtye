// The screens of the hosts (T-97, T-98): the rail's projects across hosts with
// the mark of a remote one, the banner and the footer of a remote host on
// screen. The registry and the stores are real; the shell and the remote hosts
// are fakes (src/test/remoteHosts.ts).

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RemoteDevice } from "../api/generated";
import { LOCAL_HOST } from "../store/hosts";
import { HostsProvider } from "../store/useHosts";
import { FAR_ONLY, FAR_REPO, FakeHostsApi, hostInfo, newTestHosts, type TestHosts } from "../test/remoteHosts";
import { ConnectionBanner } from "./ConnectionBanner";
import { IconRail } from "./IconRail";
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

/** The rail on the hosts' registry; `onChange` is the view switch it asks for. */
function mountRail() {
  const onChange = vi.fn();
  const user = mount(<IconRail view="work" onChange={onChange} settingsOpen={false} onOpenSettings={() => {}} />);
  return { user, onChange };
}

const marks = (within_: HTMLElement) => within(within_).queryAllByRole("img", { name: /^リモート/ });

beforeEach(() => {
  t = newTestHosts();
});

afterEach(() => {
  t.registry.stop();
});

describe("the rail's projects across hosts", () => {
  /** Every remote host has both its projects open, so that the rail lists them. */
  function withOpenFarProjects() {
    t.registry.stop();
    t = newTestHosts({
      configureFar: (far) => {
        far.core.info = { ...FAR_REPO, open: true };
        far.core.others = [{ ...FAR_ONLY, open: true, awake: true }];
      },
    });
  }
  const rows = () => within(screen.getByRole("list", { name: "開いているプロジェクト" })).getAllByRole("button");
  const row = (name: RegExp) => within(screen.getByRole("list", { name: "開いているプロジェクト" })).getByRole("button", { name });

  it("lists this PC's projects first, then each remote host's, a remote one marked with its host", async () => {
    withOpenFarProjects();
    mountRail();
    await started(["sid-1", "sid-2"]);
    await waitFor(() => expect(rows()).toHaveLength(6));
    expect(rows().map((o) => o.textContent)).toEqual([
      "repo",
      "beta",
      "repohost-sid-1",
      "server-onlyhost-sid-1",
      "repohost-sid-2",
      "server-onlyhost-sid-2",
    ]);
    expect(rows().map((o) => marks(o).length)).toEqual([0, 0, 1, 1, 1, 1]);
    expect(rows()[2]).toHaveAttribute("title", expect.stringContaining("/srv/repo"));
  });

  it("lists only the projects that are open: a remote host's others wait on the dashboard", async () => {
    mountRail();
    await started(["sid-1"]);
    expect(rows().map((o) => o.textContent)).toEqual(["repo", "beta"]);
  });

  it("marks a remote project with the host's name and state on hover", async () => {
    withOpenFarProjects();
    mountRail();
    await started();
    await waitFor(() => expect(rows()).toHaveLength(4));
    const mark = marks(rows()[3])[0];
    expect(mark).toHaveAttribute("title", "host-sid-1 · 接続中");
    expect(mark).not.toHaveClass("down");
    expect(mark.querySelector("svg")).toHaveAttribute("data-icon", "remote");
  });

  it("greys the mark of a host that is not connected, and does not pick its project", async () => {
    withOpenFarProjects();
    const { onChange } = mountRail();
    await started();
    await waitFor(() => expect(rows()).toHaveLength(4));
    (t.api as FakeHostsApi).notice({
      type: "status",
      server_id: "sid-1",
      status: { connection: { state: "waiting_to_reconnect", attempt: 1, delay_ms: 1000, reason: "refused" }, server: null },
    });
    const far = row(/^server-only に切り替え/);
    await waitFor(() => expect(far).toHaveAttribute("aria-disabled", "true"));
    const mark = marks(far)[0];
    expect(mark).toHaveClass("down");
    expect(mark).toHaveAttribute("title", "host-sid-1 · 切断 (再接続待ち)");
    const user = userEvent.setup();
    await user.click(far);
    expect(t.registry.getState().active).toBe(LOCAL_HOST);
    expect(t.fars.get("sid-1")?.transport.callsOf("open_project")).toEqual([]);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("looks as it did when no host is paired with: no mark, no host name", async () => {
    mountRail();
    await started([]);
    expect(rows().map((o) => o.textContent)).toEqual(["repo", "beta"]);
    expect(screen.queryAllByRole("img", { name: /^リモート/ })).toEqual([]);
    expect(document.querySelector(".host-tag, .remote-mark")).toBeNull();
  });

  it("switches the screen to the host of the project that is clicked, and opens it there", async () => {
    withOpenFarProjects();
    const { user, onChange } = mountRail();
    await started();
    await waitFor(() => expect(rows()).toHaveLength(4));
    await user.click(row(/^server-only に切り替え/));
    expect(onChange).toHaveBeenCalledWith("work");
    expect(t.registry.getState().active).toBe("sid-1");
    expect(t.fars.get("sid-1")?.transport.callsOf("open_project").map((c) => c.path)).toEqual([FAR_ONLY.path]);
    expect(t.localTransport.callsOf("open_project")).toEqual([]);
    // The rail follows: the project is the current one now, and only it.
    await waitFor(() => expect(row(/^server-only に切り替え/)).toHaveAttribute("aria-current", "true"));
    expect(rows().filter((r) => r.getAttribute("aria-current") === "true")).toHaveLength(1);
  });

  it("keeps the same project of another host apart from the one on screen", async () => {
    withOpenFarProjects();
    mountRail();
    await started();
    await act(() => t.registry.selectProject(LOCAL_HOST, t.registry.getState().projects[0].project));
    await waitFor(() => expect(t.localTransport.callsOf("open_project")).toHaveLength(1));
    await waitFor(() => expect(rows()).toHaveLength(4));
    // Two rows are called repo; only this app's is the open one.
    const repos = rows().filter((r) => r.getAttribute("aria-label")?.startsWith("repo に切り替え"));
    expect(repos.map((o) => o.getAttribute("aria-current"))).toEqual(["true", null]);
    expect(marks(repos[0])).toEqual([]);
    expect(marks(repos[1])).toHaveLength(1);
  });

  it("gives the status dot of a remote project by that host's word, and of the same id by its own", async () => {
    t.registry.stop();
    t = newTestHosts({
      configureFar: (far) => {
        far.core.info = { ...FAR_REPO, open: true, running: true, awake: true };
      },
    });
    mountRail();
    await started();
    await waitFor(() => expect(rows()).toHaveLength(3));
    expect(screen.getByTestId("rail-dot-host:sid-1:repo")).toHaveClass("project-dot-running");
    expect(screen.getByTestId("rail-dot-repo")).toHaveClass("project-dot-idle");
  });
});

describe("sleeping and moving projects in the rail across hosts", () => {
  function withOpenFarProjects() {
    t.registry.stop();
    t = newTestHosts({
      configureFar: (far) => {
        far.core.info = { ...FAR_REPO, open: true };
        far.core.others = [{ ...FAR_ONLY, open: true, awake: true }];
      },
    });
  }
  const rows = () => within(screen.getByRole("list", { name: "開いているプロジェクト" })).getAllByRole("button");

  it("offers the menu and the drag on the rows of this PC only while this PC is on screen", async () => {
    withOpenFarProjects();
    const { user } = mountRail();
    await started();
    await waitFor(() => expect(rows()).toHaveLength(4));
    // repo, beta (this PC's), then repo, server-only (the host's).
    expect(rows().map((o) => o.closest("li")?.getAttribute("draggable") ?? null)).toEqual(["true", "true", null, null]);

    await user.pointer({ keys: "[MouseRight]", target: rows()[3] });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    await user.pointer({ keys: "[MouseRight]", target: rows()[1] });
    const menu = screen.getByRole("menu", { name: "beta の操作" });
    await user.click(within(menu).getByRole("menuitem", { name: "上へ移動" }));
    expect(t.localTransport.callsOf("reorder_projects").at(-1)?.ids).toEqual(["beta", "repo"]);
    expect(t.fars.get("sid-1")?.transport.callsOf("reorder_projects")).toEqual([]);
  });

  it("sends the move of a remote host's project to that host, and leaves this PC's rows alone", async () => {
    withOpenFarProjects();
    const { user } = mountRail();
    await started();
    await showRemote();
    await waitFor(() => expect(rows()).toHaveLength(4));
    // The host's own rows are the ones that can be moved now.
    expect(rows().map((o) => o.closest("li")?.getAttribute("draggable") ?? null)).toEqual([null, null, "true", "true"]);

    await user.pointer({ keys: "[MouseRight]", target: rows()[0] });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    // Opening server-only used it: it is the host's first.
    expect(rows()[2]).toHaveAttribute("aria-label", expect.stringMatching(/^server-only /));
    await user.pointer({ keys: "[MouseRight]", target: rows()[2] });
    const menu = screen.getByRole("menu", { name: "server-only の操作" });
    expect(within(menu).getByRole("menuitem", { name: "上へ移動" })).toBeDisabled();
    await user.click(within(menu).getByRole("menuitem", { name: "下へ移動" }));
    expect(t.fars.get("sid-1")?.transport.callsOf("reorder_projects").at(-1)?.ids).toEqual(["repo", "server-only"]);
    expect(t.localTransport.callsOf("reorder_projects")).toEqual([]);
  });
});

describe("the sidebar's project of a remote host", () => {
  it("names the project with the remote mark, and this PC's without one", async () => {
    mount(<Sidebar />);
    await started();
    await act(() => t.registry.selectProject(LOCAL_HOST, t.registry.getState().projects[0].project));
    await waitFor(() => expect(document.querySelector(".project-current .name")).toHaveTextContent("repo"));
    expect(marks(document.querySelector(".project-current") as HTMLElement)).toEqual([]);

    await showRemote();
    await waitFor(() => expect(document.querySelector(".project-current .name")).toHaveTextContent("server-only"));
    const current = document.querySelector(".project-current") as HTMLElement;
    expect(marks(current)).toHaveLength(1);
    expect(current).toHaveAttribute("title", expect.stringContaining("host-sid-1: /srv/server-only"));
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
