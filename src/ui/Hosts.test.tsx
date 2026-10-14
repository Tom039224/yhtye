// The screens of the hosts (T-97): the project pull-down across hosts, and the
// question that a pairing link asks. The registry and the stores are real; the
// shell and the remote hosts are fakes (src/test/remoteHosts.ts).

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import App from "../App";
import { CommandError } from "../api/transport";
import { LOCAL_HOST } from "../store/hosts";
import { HostsProvider } from "../store/useHosts";
import { FAR_ONLY, FakeHostsApi, hostInfo, newTestHosts, type TestHosts } from "../test/remoteHosts";
import { PairLinkDialog } from "./PairLinkDialog";
import { Sidebar } from "./Sidebar";

const LINK = "yhtye://pair?v=1&ip=100.64.0.7&port=1423&code=ABCD2345&sid=sid-new";

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

beforeEach(() => {
  t = newTestHosts();
});

afterEach(() => {
  t.registry.stop();
});

describe("the project pull-down across hosts", () => {
  it("lists the projects of every host, a remote one with its host's name", async () => {
    const user = mount(<Sidebar />);
    await started();
    await user.click(screen.getByRole("button", { name: "プロジェクトを選択" }));
    const options = within(screen.getByRole("listbox", { name: "プロジェクト" })).getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual(["repo", "beta", "repohost-sid-1", "server-onlyhost-sid-1"]);
    // The first two are this app's own; the project that is the same name on the server is another row.
    expect(within(options[0]).queryByText("host-sid-1")).not.toBeInTheDocument();
    expect(options[2]).toHaveAttribute("title", expect.stringContaining("/srv/repo"));
  });

  it("switches the screen to the host of the project that is picked, and opens it there", async () => {
    const user = mount(<Sidebar />);
    await started();
    await user.click(screen.getByRole("button", { name: "プロジェクトを選択" }));
    await user.click(screen.getByRole("option", { name: /server-only/ }));
    expect(t.registry.getState().active).toBe("sid-1");
    await waitFor(() => expect(screen.getByRole("button", { name: "server-only" })).toBeInTheDocument());
    expect(t.fars.get("sid-1")?.transport.callsOf("open_project").map((c) => c.path)).toEqual([FAR_ONLY.path]);
    expect(t.localTransport.callsOf("open_project")).toEqual([]);

    // The list is still every host's, and the open one is marked.
    await user.click(screen.getByRole("button", { name: "server-only" }));
    const options = screen.getAllByRole("option");
    expect(options.map((o) => o.getAttribute("aria-selected"))).toEqual(["false", "false", "false", "true"]);
  });

  it("does not pick a project of a host that is not connected", async () => {
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
    await user.click(option);
    expect(t.registry.getState().active).toBe(LOCAL_HOST);
    expect(t.fars.get("sid-1")?.transport.callsOf("open_project")).toEqual([]);
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
  });
});

describe("a pairing link", () => {
  it("asks first: the host and the name of this device, and pairs only on the answer", async () => {
    const user = mount(<PairLinkDialog registry={t.registry} />);
    await started([]);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    act(() => t.registry.addPairLink(LINK));
    const dialog = await screen.findByRole("dialog", { name: "別の Yhtye に接続" });
    expect(await within(dialog).findByText("100.64.0.7")).toBeInTheDocument();
    expect(within(dialog).getByText("1423")).toBeInTheDocument();
    const name = within(dialog).getByRole("textbox", { name: "この PC の名前" });
    expect(name).toHaveValue("this pc");
    expect((t.api as FakeHostsApi).calls.some((c) => c.startsWith("add"))).toBe(false);

    await user.clear(name);
    await user.type(name, "my laptop");
    await user.click(within(dialog).getByRole("button", { name: "接続する" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect((t.api as FakeHostsApi).calls).toContain("add uri my laptop");
    expect(t.registry.getState().hosts.map((h) => h.key)).toEqual([LOCAL_HOST, "sid-new"]);
    expect(t.registry.getState().pairLinks).toEqual([]);
  });

  it("pairs nothing when declined, by the button or Esc", async () => {
    const user = mount(<PairLinkDialog registry={t.registry} />);
    await started([]);
    act(() => t.registry.addPairLink(LINK));
    await user.click(await screen.findByRole("button", { name: "キャンセル" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    act(() => t.registry.addPairLink(LINK));
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText("100.64.0.7");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect((t.api as FakeHostsApi).calls.some((c) => c.startsWith("add"))).toBe(false);
    expect(t.registry.getState().hosts).toHaveLength(1);
  });

  it("says why pairing failed, and can be tried again", async () => {
    const user = mount(<PairLinkDialog registry={t.registry} />);
    await started([]);
    const api = t.api as FakeHostsApi;
    api.addError = new CommandError("forbidden", "the pairing code was refused");
    act(() => t.registry.addPairLink(LINK));
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText("100.64.0.7");
    await user.click(within(dialog).getByRole("button", { name: "接続する" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("the pairing code was refused");
    expect(t.registry.getState().pairLinks).toEqual([LINK]);
    api.addError = null;
    await user.click(within(dialog).getByRole("button", { name: "接続する" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("says when the link cannot be used", async () => {
    const user = mount(<PairLinkDialog registry={t.registry} />);
    await started([]);
    const api = t.api as FakeHostsApi;
    api.previewPairing = async () => {
      throw new CommandError("invalid_argument", "192.168.1.5 is not a Tailscale address");
    };
    act(() => t.registry.addPairLink("yhtye://pair?v=1&ip=192.168.1.5"));
    expect(await screen.findByRole("alert")).toHaveTextContent("192.168.1.5 is not a Tailscale address");
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "閉じる" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("asks about the next link once the first is answered", async () => {
    const user = mount(<PairLinkDialog registry={t.registry} />);
    await started([]);
    act(() => {
      t.registry.addPairLink(LINK);
      t.registry.addPairLink(LINK + "&x=2");
    });
    await user.click(await screen.findByRole("button", { name: "キャンセル" }));
    expect(t.registry.getState().pairLinks).toEqual([LINK + "&x=2"]);
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
  });

  it("is asked in the app when a link arrives from the shell", async () => {
    mount(<App />);
    await started([]);
    const api = t.api as FakeHostsApi;
    act(() => api.link(LINK));
    expect(await screen.findByRole("dialog", { name: "別の Yhtye に接続" })).toBeInTheDocument();
    expect(api.calls.some((c) => c.startsWith("add"))).toBe(false);
  });
});
