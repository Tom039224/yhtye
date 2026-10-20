// The settings' "接続先" section (T-98): pairing with another host from a link or by typing,
// the hosts paired with, a link that arrives from outside, and what the settings say
// of the remote access of a remote host on screen. The registry and the stores are real;
// the shell and the remote hosts are fakes (src/test/remoteHosts.ts).

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import App from "../App";
import { CommandError } from "../api/transport";
import { LOCAL_HOST } from "../store/hosts";
import { HostsProvider } from "../store/useHosts";
import { StoreContext } from "../store/useStore";
import { FAR_ONLY, FakeHostsApi, hostInfo, newTestHosts, type TestHosts } from "../test/remoteHosts";
import { SettingsModal } from "./SettingsModal";

const LINK = "yhtye://pair?v=1&ip=100.64.0.7&port=1423&code=ABCD2345&sid=sid-new";

let t: TestHosts;

const api = () => t.api as FakeHostsApi;

function mount(ui: React.ReactElement = <App />) {
  render(<HostsProvider registry={t.registry}>{ui}</HostsProvider>);
  return userEvent.setup();
}

async function started(hosts: string[] = []) {
  api().hosts = hosts.map((id) => hostInfo(id));
  await act(() => t.registry.start());
  await waitFor(() => expect(t.registry.getState().projects.length).toBeGreaterThanOrEqual(2 + hosts.length * 2));
}

/** Opens the settings from the rail, and one of its sections. */
async function openSection(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(await screen.findByRole("button", { name: "設定" }));
  const dialog = await screen.findByRole("dialog", { name: "設定" });
  await user.click(within(dialog).getByRole("button", { name }));
  return dialog;
}

const hostRow = (dialog: HTMLElement, name: string) => within(dialog).getByRole("listitem", { name });
const added = () => api().calls.filter((c) => c.startsWith("add"));

beforeEach(() => {
  t = newTestHosts();
});

afterEach(() => {
  t.registry.stop();
});

describe("pairing with a host from a link", () => {
  it("shows where the link leads, and pairs only when 接続 is pressed, under the name confirmed", async () => {
    const user = mount();
    await started();
    const dialog = await openSection(user, "接続先");
    await user.click(within(dialog).getByRole("textbox", { name: "ペアリングリンク" }));
    await user.paste(LINK);
    const facts = await within(dialog).findByLabelText("リンクの接続先");
    expect(within(facts).getByText("100.64.0.7")).toBeInTheDocument();
    expect(within(facts).getByText("1423")).toBeInTheDocument();
    const name = within(dialog).getByRole("textbox", { name: "この PC の名前" });
    expect(name).toHaveValue("this pc");
    expect(added()).toEqual([]);

    await user.clear(name);
    await user.type(name, "my laptop");
    await user.click(within(dialog).getByRole("button", { name: "接続" }));
    await waitFor(() => expect(added()).toEqual(["add uri my laptop"]));
    expect(api().inputs[0]).toEqual({ kind: "uri", uri: LINK });
    expect(await within(dialog).findByText(/host-sid-new に接続しました/)).toBeInTheDocument();
    expect(t.registry.getState().hosts.map((h) => h.key)).toEqual([LOCAL_HOST, "sid-new"]);
    expect(hostRow(dialog, "host-sid-new")).toHaveTextContent("my laptop");
    // The link is spent: the box is empty again.
    expect(within(dialog).getByRole("textbox", { name: "ペアリングリンク" })).toHaveValue("");
  });

  it("offers the name of this PC until another is typed", async () => {
    const user = mount();
    await started();
    const dialog = await openSection(user, "接続先");
    // No link yet: the name of the machine this app runs on, as its core reported.
    await waitFor(() => expect(within(dialog).getByRole("textbox", { name: "この PC の名前" })).toHaveValue("test-host"));
  });

  it("does not offer 接続 without a link, or with a link that cannot be used", async () => {
    const user = mount();
    await started();
    const dialog = await openSection(user, "接続先");
    const connect = within(dialog).getByRole("button", { name: "接続" });
    expect(connect).toBeDisabled();
    api().previewPairing = async () => {
      throw new CommandError("invalid_argument", "192.168.1.5 is not a Tailscale address");
    };
    await user.click(within(dialog).getByRole("textbox", { name: "ペアリングリンク" }));
    await user.paste("yhtye://pair?v=1&ip=192.168.1.5");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("このリンクは使えません: 192.168.1.5 は Tailscale のアドレスではありません");
    expect(connect).toBeDisabled();
    expect(added()).toEqual([]);
  });
});

describe("pairing with a host by typing", () => {
  it("takes the host, the port and the code", async () => {
    const user = mount();
    await started();
    const dialog = await openSection(user, "接続先");
    await user.click(within(dialog).getByRole("button", { name: "ホスト・ポート・コード" }));
    expect(within(dialog).queryByRole("textbox", { name: "ペアリングリンク" })).not.toBeInTheDocument();
    expect(within(dialog).getByRole("textbox", { name: "ポート" })).toHaveAttribute("placeholder", "1423");
    expect(within(dialog).getByRole("button", { name: "接続" })).toBeDisabled();
    await user.type(within(dialog).getByRole("textbox", { name: "ホスト" }), " 100.64.0.7 ");
    await user.type(within(dialog).getByRole("textbox", { name: "コード" }), "abcd-2345");
    await user.click(within(dialog).getByRole("button", { name: "接続" }));
    await waitFor(() => expect(added()).toHaveLength(1));
    expect(api().inputs[0]).toEqual({ kind: "manual", host: "100.64.0.7", port: "", code: "abcd-2345" });
  });
});

describe("a pairing that fails", () => {
  it.each([
    ["forbidden", "the pairing code was refused", "コードが違うか、有効期限が切れたか"],
    ["forbidden", "the host does not take devices of other Tailscale users", "同じ Tailscale ユーザーの端末でない"],
    ["conflict", "another Yhtye answered (expected a, got b)", "別の Yhtye が答えました"],
    ["unavailable", "could not reach 100.64.0.7:1423", "接続先に届きません"],
    ["invalid_argument", "100.64.0.7 is not a Tailscale address", "Tailscale のアドレスではありません"],
  ] as const)("says in words why (%s: %s), and can be tried again", async (code, message, words) => {
    const user = mount();
    await started();
    const dialog = await openSection(user, "接続先");
    api().addError = new CommandError(code, message);
    await user.click(within(dialog).getByRole("button", { name: "ホスト・ポート・コード" }));
    await user.type(within(dialog).getByRole("textbox", { name: "ホスト" }), "100.64.0.7");
    await user.type(within(dialog).getByRole("textbox", { name: "コード" }), "ABCD2345");
    await user.click(within(dialog).getByRole("button", { name: "接続" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(words);
    expect(t.registry.getState().hosts).toHaveLength(1);
    expect(within(dialog).getByRole("textbox", { name: "コード" })).toHaveValue("ABCD2345");

    api().addError = null;
    await user.click(within(dialog).getByRole("button", { name: "接続" }));
    await waitFor(() => expect(t.registry.getState().hosts).toHaveLength(2));
    expect(within(dialog).queryByRole("alert")).not.toBeInTheDocument();
  });

  it("warns, when pairing worked, that the token could not be kept, and the host must be paired with again after a restart", async () => {
    const user = mount();
    await started();
    const dialog = await openSection(user, "接続先");
    api().addedToken = { place: "memory_only", reason: "no secret service" };
    await user.click(within(dialog).getByRole("textbox", { name: "ペアリングリンク" }));
    await user.paste(LINK);
    await user.click(within(dialog).getByRole("button", { name: "接続" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("キーリングが使えない");
    expect(alert).toHaveTextContent("再起動すると、このホストにはペアリングし直す必要があります");
    // And it stays beside the host in the list.
    expect(hostRow(dialog, "host-sid-new")).toHaveTextContent("ペアリングし直す必要があります");
  });
});

describe("the hosts paired with", () => {
  it("lists each with its state, its address and the name this PC is listed under there", async () => {
    const user = mount();
    await started(["sid-1", "sid-2"]);
    api().notice({
      type: "status",
      server_id: "sid-2",
      status: { connection: { state: "waiting_to_reconnect", attempt: 2, delay_ms: 5000, reason: "refused" }, server: null },
    });
    const dialog = await openSection(user, "接続先");
    const list = within(dialog).getByRole("list", { name: "接続先の一覧" });
    expect(within(list).getAllByRole("listitem").map((li) => li.getAttribute("aria-label"))).toEqual(["host-sid-1", "host-sid-2"]);
    expect(hostRow(dialog, "host-sid-1")).toHaveTextContent("接続中");
    expect(hostRow(dialog, "host-sid-1")).toHaveTextContent("100.64.0.7:1423");
    expect(hostRow(dialog, "host-sid-1")).toHaveTextContent("端末名 this pc");
    await waitFor(() => expect(hostRow(dialog, "host-sid-2")).toHaveTextContent("切断 (再接続待ち)"));
  });

  it("says a host that removed this device did so, and offers no reconnecting to it", async () => {
    const user = mount();
    await started(["sid-1"]);
    api().notice({ type: "status", server_id: "sid-1", status: { connection: { state: "revoked" }, server: null } });
    const dialog = await openSection(user, "接続先");
    await waitFor(() => expect(hostRow(dialog, "host-sid-1")).toHaveTextContent("このホストから外されました"));
    expect(within(hostRow(dialog, "host-sid-1")).queryByRole("button", { name: /再接続/ })).not.toBeInTheDocument();
    expect(within(hostRow(dialog, "host-sid-1")).getByRole("button", { name: "host-sid-1 を削除" })).toBeInTheDocument();
  });

  it("connects again on request, when the host is not connected", async () => {
    const user = mount();
    await started(["sid-1"]);
    const dialog = await openSection(user, "接続先");
    expect(within(hostRow(dialog, "host-sid-1")).queryByRole("button", { name: /再接続/ })).not.toBeInTheDocument();
    api().notice({
      type: "status",
      server_id: "sid-1",
      status: { connection: { state: "waiting_to_reconnect", attempt: 1, delay_ms: 5000, reason: "refused" }, server: null },
    });
    await user.click(await within(dialog).findByRole("button", { name: "host-sid-1 に再接続" }));
    expect(api().calls).toContain("connect sid-1");
  });

  it("deletes a host after asking, and says it stays on the host's own list", async () => {
    const user = mount();
    await started(["sid-1"]);
    const dialog = await openSection(user, "接続先");
    await user.click(within(dialog).getByRole("button", { name: "host-sid-1 を削除" }));
    expect(api().calls.some((c) => c.startsWith("remove"))).toBe(false);
    await user.click(within(dialog).getByRole("button", { name: "やめる" }));
    expect(within(dialog).getByRole("button", { name: "host-sid-1 を削除" })).toBeInTheDocument();

    await user.click(within(dialog).getByRole("button", { name: "host-sid-1 を削除" }));
    expect(within(dialog).getByText(/ホスト側の端末一覧には残ります/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "削除する" }));
    await waitFor(() => expect(api().calls).toContain("remove sid-1"));
    await waitFor(() => expect(within(dialog).getByText("接続先はありません。")).toBeInTheDocument());
    expect(within(dialog).getByRole("status")).toHaveTextContent("host-sid-1 を削除しました");
    expect(t.registry.getState().hosts.map((h) => h.key)).toEqual([LOCAL_HOST]);
  });
});

describe("a pairing link that arrives from outside", () => {
  it("opens the settings on 接続先 with the link in the box, and pairs nothing until 接続", async () => {
    const user = mount();
    await started();
    expect(screen.queryByRole("dialog", { name: "設定" })).not.toBeInTheDocument();
    act(() => api().link(LINK));
    const dialog = await screen.findByRole("dialog", { name: "設定" });
    expect(within(dialog).getByRole("button", { name: "接続先" })).toHaveAttribute("aria-current", "page");
    expect(within(dialog).getByRole("textbox", { name: "ペアリングリンク" })).toHaveValue(LINK);
    const facts = await within(dialog).findByLabelText("リンクの接続先");
    expect(within(facts).getByText("100.64.0.7")).toBeInTheDocument();
    expect(within(dialog).getByRole("textbox", { name: "この PC の名前" })).toHaveValue("this pc");
    expect(added()).toEqual([]);
    expect(t.registry.getState().pairLinks).toEqual([]);

    await user.click(within(dialog).getByRole("button", { name: "接続" }));
    await waitFor(() => expect(added()).toEqual(["add uri this pc"]));
  });

  it("switches an open settings modal to 接続先", async () => {
    const user = mount();
    await started();
    const dialog = await openSection(user, "ハーネス");
    expect(within(dialog).getByRole("button", { name: "ハーネス" })).toHaveAttribute("aria-current", "page");
    act(() => api().link(LINK));
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "接続先" })).toHaveAttribute("aria-current", "page"));
    expect(within(dialog).getByRole("textbox", { name: "ペアリングリンク" })).toHaveValue(LINK);
    expect(screen.getAllByRole("dialog", { name: "設定" })).toHaveLength(1);
  });

  it("puts the latest link in the box when another follows, and a link that came before the screen was ready too", async () => {
    api().waiting = [LINK];
    const user = mount();
    await started();
    const dialog = await screen.findByRole("dialog", { name: "設定" });
    expect(within(dialog).getByRole("textbox", { name: "ペアリングリンク" })).toHaveValue(LINK);
    const next = LINK.replace("ABCD2345", "WXYZ6789");
    act(() => api().link(next));
    await waitFor(() => expect(within(dialog).getByRole("textbox", { name: "ペアリングリンク" })).toHaveValue(next));

    // Closed, the settings open the way they always do.
    await user.click(within(dialog).getByRole("button", { name: "閉じる" }));
    await user.click(await screen.findByRole("button", { name: "設定" }));
    const again = await screen.findByRole("dialog", { name: "設定" });
    expect(within(again).getByRole("button", { name: "エージェント" })).toHaveAttribute("aria-current", "page");
  });
});

describe("the remote access of a remote host on screen", () => {
  async function remoteOnScreen() {
    const user = mount();
    await started(["sid-1"]);
    await act(() => t.registry.selectProject("sid-1", FAR_ONLY));
    await waitFor(() => expect(t.registry.getState().active).toBe("sid-1"));
    return user;
  }

  it("is explained instead of shown: it is run by the host itself", async () => {
    const user = await remoteOnScreen();
    const dialog = await openSection(user, "リモート接続");
    const note = within(dialog).getByRole("status");
    expect(note).toHaveTextContent("host-sid-1 のリモート接続の設定・端末の追加と失効は、そのホスト自身でしか行えません");
    expect(note).toHaveTextContent("yhtye remote pair");
    expect(within(dialog).queryByRole("switch")).not.toBeInTheDocument();
    // Neither this PC's nor the host's server was asked about.
    expect(t.fars.get("sid-1")?.transport.callsOf("get_remote_status")).toEqual([]);
    expect(t.localTransport.callsOf("get_remote_status")).toEqual([]);
  });

  it("puts the host's name by the heading of the settings that are the host's, and only of those", async () => {
    const user = await remoteOnScreen();
    const dialog = await openSection(user, "エージェント");
    const host = within(dialog).getByTitle(/host-sid-1 のものです/);
    expect(host).toHaveTextContent("host-sid-1の設定");
    await user.click(within(dialog).getByRole("button", { name: "ハーネス" }));
    expect(within(dialog).getByTitle(/host-sid-1 のものです/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "接続先" }));
    expect(within(dialog).queryByTitle(/host-sid-1 のものです/)).not.toBeInTheDocument();
  });

  it("is shown as always for this PC's own, with nothing by the heading", async () => {
    const user = mount();
    await started(["sid-1"]);
    const dialog = await openSection(user, "リモート接続");
    expect(await within(dialog).findByRole("switch", { name: "リモート接続" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "エージェント" }));
    expect(within(dialog).queryByTitle(/のものです/)).not.toBeInTheDocument();
  });

  it("is explained too where the host answers that it is not for a remote client to run", async () => {
    t.localCore.failures.set("get_remote_status", new CommandError("forbidden", "not for remote clients"));
    const user = mount();
    await started();
    const dialog = await openSection(user, "リモート接続");
    const note = await within(dialog).findByRole("status");
    expect(note).toHaveTextContent("そのホスト自身でしか行えません");
    expect(within(dialog).queryByRole("alert")).not.toBeInTheDocument();
  });
});

describe("without the shell's host management", () => {
  it("has no 接続先 in the settings (a browser on the dev bridge)", async () => {
    t.registry.stop();
    t = newTestHosts({ api: null });
    await act(() => t.registry.start());
    render(
      <StoreContext.Provider value={t.registry.activeStore}>
        <SettingsModal project={null} onClose={() => {}} />
      </StoreContext.Provider>,
    );
    expect(screen.queryByRole("button", { name: "接続先" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "リモート接続" })).toBeInTheDocument();
  });
});
