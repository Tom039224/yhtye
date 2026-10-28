// The dashboard: the welcome, the projects of every host and "プロジェクトを追加"
// (the OS dialog on this PC, a folder browser on a remote host or in a browser),
// and the usage statistics of the host on screen with their states.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ProjectInfo, UsageStats } from "../api/generated";
import { CommandError } from "../api/transport";
import { AppStore } from "../store/app";
import { LOCAL_HOST } from "../store/hosts";
import { HostsProvider } from "../store/useHosts";
import { StoreContext } from "../store/useStore";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { cannedUsageStats, FakeCore, MemoryTransport } from "../test/memoryTransport";
import { FAR_ONLY, FakeHostsApi, hostInfo, newTestHosts } from "../test/remoteHosts";
import { AGENTSVIEW_URL, Dashboard } from "./Dashboard";

const picker = vi.hoisted(() => ({ canPickLocalFolder: vi.fn<(remote: boolean) => boolean>(), pickLocalFolder: vi.fn<(title: string) => Promise<string | null>>() }));
vi.mock("../api/folderPicker", () => picker);

const BETA: ProjectInfo = { id: "beta", name: "beta", path: "/tmp/beta", open: true, running: false, awake: true };
const GAMMA: ProjectInfo = { id: "gamma", name: "gamma", path: "/tmp/gamma", open: false, running: false, awake: false };

/** The OS dialog is there (as in the app on this PC), or not (a browser). */
function nativeDialog(available: boolean) {
  picker.canPickLocalFolder.mockImplementation((remote) => available && !remote);
}

beforeEach(() => {
  picker.canPickLocalFolder.mockReset();
  picker.pickLocalFolder.mockReset();
  nativeDialog(false);
});

afterEach(() => {
  vi.restoreAllMocks();
});

function setup(opts: { others?: ProjectInfo[]; connected?: boolean; usageStats?: UsageStats; noProjects?: boolean } = {}) {
  const transport = new MemoryTransport(opts.connected === false ? { state: "connecting" } : { state: "open" });
  const core = new FakeCore(PROJECT, FULL_RUN.start);
  core.others = opts.others ?? [BETA, GAMMA];
  if (opts.usageStats) core.usageStats = opts.usageStats;
  core.attach(transport);
  if (opts.noProjects) {
    // The core knows no project yet (set before the store asks for the list).
    const serve = transport.handler;
    transport.handler = (cmd) => (cmd.type === "list_projects" ? { type: "projects", projects: [] } : serve(cmd));
  }
  const store = new AppStore(transport, { reopenLast: false });
  store.start();
  const onOpenWork = vi.fn();
  render(
    <StoreContext.Provider value={store}>
      <Dashboard onOpenWork={onOpenWork} />
    </StoreContext.Provider>,
  );
  return { transport, core, store, onOpenWork, user: userEvent.setup() };
}

const projectList = () => screen.findByRole("list", { name: "登録済みのプロジェクト" });
const projectRow = (name: string) => screen.findByRole("button", { name: new RegExp(`^${name} を開く`) });
const addButton = () => screen.getByRole("button", { name: "プロジェクトを追加" });
const cards = async () => within(await screen.findByRole("list", { name: "期間の合計" }));

describe("the welcome", () => {
  it("greets, and credits agentsview with a link that opens outside the app", () => {
    setup();
    expect(screen.getByRole("heading", { name: "ようこそ", level: 1 })).toBeInTheDocument();
    const credit = screen.getByText(/利用統計は/, { selector: "footer" });
    expect(credit).toHaveTextContent("利用統計は agentsview (MIT) を参考にしています");
    const link = within(credit).getByRole("link", { name: "agentsview" });
    expect(link).toHaveAttribute("href", AGENTSVIEW_URL);
    expect(AGENTSVIEW_URL).toBe("https://github.com/kenn-io/agentsview");
    expect(link).toHaveAttribute("rel", expect.stringContaining("noopener"));
  });

  it("opens the credit link in a new window, never in the app", async () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    const { user } = setup();
    await user.click(screen.getByRole("link", { name: "agentsview" }));
    expect(open).toHaveBeenCalledWith(AGENTSVIEW_URL, "_blank", "noopener,noreferrer");
  });
});

describe("the projects", () => {
  it("lists every registered project, open or not, most recently used first, each with its state", async () => {
    setup();
    const list = await projectList();
    await waitFor(() => expect(within(list).getAllByRole("button")).toHaveLength(3));
    const rows = within(list).getAllByRole("button");
    expect(rows.map((r) => r.getAttribute("aria-label"))).toEqual(["repo を開く (停止)", "beta を開く (待機中)", "gamma を開く (停止)"]);
    expect(within(rows[1]).getByText("/tmp/beta")).toBeInTheDocument();
    expect(within(rows[1]).getByText("待機中")).toBeInTheDocument();
    expect(screen.getByTestId("dash-dot-beta")).toHaveClass("project-dot-idle");
    expect(screen.getByTestId("dash-dot-beta")).toHaveAttribute("data-ring", "ready");
  });

  it("lights the dot of a project with work on", async () => {
    setup({ others: [{ ...BETA, running: true }, GAMMA] });
    await projectRow("beta");
    expect(screen.getByTestId("dash-dot-beta")).toHaveClass("project-dot-running");
  });

  it("opens the project that is clicked and switches to the work view", async () => {
    const { user, transport, onOpenWork, store } = setup();
    await user.click(await projectRow("gamma"));
    expect(transport.callsOf("open_project").at(-1)?.path).toBe("/tmp/gamma");
    expect(onOpenWork).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(store.getState().project?.info.id).toBe("gamma"));
    await waitFor(() => expect(screen.getByRole("button", { name: /^gamma を開く/ })).toHaveAttribute("aria-current", "true"));
  });

  it("goes to the work view without opening the project again when it is on screen", async () => {
    const { user, transport, onOpenWork, store } = setup();
    await act(() => store.openProject(PROJECT.path));
    const opened = transport.callsOf("open_project").length;
    await user.click(await projectRow("repo"));
    expect(transport.callsOf("open_project")).toHaveLength(opened);
    expect(onOpenWork).toHaveBeenCalledTimes(1);
  });

  it("invites to add a project while there is none", async () => {
    setup({ noProjects: true });
    expect(await screen.findByText(/まだプロジェクトがありません/)).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "登録済みのプロジェクト" })).not.toBeInTheDocument();
    expect(addButton()).toBeInTheDocument();
  });
});

describe("adding a project with the OS dialog", () => {
  beforeEach(() => nativeDialog(true));

  it("opens the folder that is picked and switches to the work view", async () => {
    picker.pickLocalFolder.mockResolvedValue("/tmp/beta");
    const { user, transport, onOpenWork } = setup();
    await user.click(addButton());
    expect(picker.pickLocalFolder).toHaveBeenCalledWith("プロジェクトのフォルダを選択");
    await waitFor(() => expect(transport.callsOf("open_project").at(-1)?.path).toBe("/tmp/beta"));
    await waitFor(() => expect(onOpenWork).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("region", { name: "フォルダを選択" })).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("does nothing when the dialog is cancelled", async () => {
    picker.pickLocalFolder.mockResolvedValue(null);
    const { user, transport, onOpenWork } = setup();
    await user.click(addButton());
    await waitFor(() => expect(picker.pickLocalFolder).toHaveBeenCalled());
    expect(transport.callsOf("open_project")).toEqual([]);
    expect(onOpenWork).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows the core's refusal under the button and not in the banners", async () => {
    picker.pickLocalFolder.mockResolvedValue("/tmp/not-a-repo");
    const { user, core, store, onOpenWork } = setup();
    core.failures.set("open_project", new CommandError("not_found", "/tmp/not-a-repo is not a git repository"));
    await user.click(addButton());
    expect(await screen.findByRole("alert")).toHaveTextContent("/tmp/not-a-repo is not a git repository");
    expect(store.getState().errors).toEqual([]);
    expect(onOpenWork).not.toHaveBeenCalled();
    // A new try clears it.
    picker.pickLocalFolder.mockResolvedValue(null);
    await user.click(addButton());
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("shows a dialog that could not be opened", async () => {
    picker.pickLocalFolder.mockRejectedValue(new Error("dialog unavailable"));
    const { user } = setup();
    await user.click(addButton());
    expect(await screen.findByRole("alert")).toHaveTextContent("dialog unavailable");
  });

  it("is not offered while the core is not connected", () => {
    setup({ connected: false });
    expect(addButton()).toBeDisabled();
  });
});

describe("adding a project with the host's folder browser", () => {
  const browser = () => screen.getByRole("region", { name: "フォルダを選択" });
  const entries = () => within(within(browser()).getByRole("list", { name: "フォルダ" })).getAllByRole("button").map((b) => b.getAttribute("aria-label"));
  const current = () => within(browser()).getByTestId("folder-current");
  const openBrowser = async (user: ReturnType<typeof userEvent.setup>) => {
    await user.click(addButton());
    await waitFor(() => expect(current()).toHaveTextContent("/home/u"));
  };

  it("lists the host's home with a parent entry and the git repositories marked", async () => {
    const { user, transport } = setup();
    expect(screen.queryByRole("region", { name: "フォルダを選択" })).not.toBeInTheDocument();
    await openBrowser(user);
    expect(picker.canPickLocalFolder).toHaveBeenCalledWith(false);
    expect(addButton()).toHaveAttribute("aria-expanded", "true");
    expect(entries()).toEqual(["親フォルダ (/home)", "Documents", "repo (git リポジトリ)"]);
    expect(transport.callsOf("list_directory")).toEqual([{ type: "list_directory" }]);
    expect(within(browser()).getByRole("textbox", { name: "フォルダのパス" })).toHaveValue("/home/u");
  });

  it("goes into a folder and back up to its parent", async () => {
    const { user } = setup();
    await openBrowser(user);
    await user.click(within(browser()).getByRole("button", { name: "repo (git リポジトリ)" }));
    await waitFor(() => expect(current()).toHaveTextContent("/home/u/repo"));
    expect(entries()).toEqual(["親フォルダ (/home/u)", "src"]);
    expect(within(browser()).getByRole("textbox", { name: "フォルダのパス" })).toHaveValue("/home/u/repo");
    await user.click(within(browser()).getByRole("button", { name: "親フォルダ (/home/u)" }));
    await waitFor(() => expect(current()).toHaveTextContent(/^\/home\/u$/));
  });

  it("says a folder has no sub-folders, and the root has no parent entry", async () => {
    const { user } = setup();
    await openBrowser(user);
    await user.click(within(browser()).getByRole("button", { name: "Documents" }));
    await waitFor(() => expect(current()).toHaveTextContent("/home/u/Documents"));
    expect(within(browser()).getByText("サブフォルダはありません。")).toBeInTheDocument();
    const field = within(browser()).getByRole("textbox", { name: "フォルダのパス" });
    await user.clear(field);
    await user.type(field, "/{Enter}");
    await waitFor(() => expect(current()).toHaveTextContent(/^\/$/));
    expect(entries()).toEqual(["home", "tmp"]);
  });

  it("jumps to a typed path, and explains one that cannot be listed while keeping the folder it was in", async () => {
    const { user } = setup();
    await openBrowser(user);
    const field = within(browser()).getByRole("textbox", { name: "フォルダのパス" });
    await user.clear(field);
    await user.type(field, "/tmp");
    await user.click(within(browser()).getByRole("button", { name: "移動" }));
    await waitFor(() => expect(current()).toHaveTextContent(/^\/tmp$/));

    await user.clear(field);
    await user.type(field, "/nope{Enter}");
    expect(await within(browser()).findByRole("alert")).toHaveTextContent("/nope does not exist");
    expect(current()).toHaveTextContent(/^\/tmp$/);
    // Going somewhere that exists clears the explanation.
    await user.clear(field);
    await user.type(field, "/home{Enter}");
    await waitFor(() => expect(within(browser()).queryByRole("alert")).not.toBeInTheDocument());
    expect(current()).toHaveTextContent(/^\/home$/);
  });

  it("opens the folder it is in and switches to the work view", async () => {
    const { user, transport, onOpenWork } = setup();
    await openBrowser(user);
    await user.click(within(browser()).getByRole("button", { name: "repo (git リポジトリ)" }));
    await waitFor(() => expect(current()).toHaveTextContent("/home/u/repo"));
    await user.click(within(browser()).getByRole("button", { name: "このフォルダを開く" }));
    await waitFor(() => expect(transport.callsOf("open_project").at(-1)?.path).toBe("/home/u/repo"));
    await waitFor(() => expect(onOpenWork).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("region", { name: "フォルダを選択" })).not.toBeInTheDocument();
  });

  it("keeps the browser open and explains why the folder could not be opened", async () => {
    const { user, core, onOpenWork, store } = setup();
    core.failures.set("open_project", new CommandError("invalid_argument", "/home/u is not inside a git repository"));
    await openBrowser(user);
    await user.click(within(browser()).getByRole("button", { name: "このフォルダを開く" }));
    expect(await within(browser()).findByRole("alert")).toHaveTextContent("/home/u is not inside a git repository");
    expect(onOpenWork).not.toHaveBeenCalled();
    expect(store.getState().errors).toEqual([]);
    expect(within(browser()).getByRole("button", { name: "このフォルダを開く" })).toBeEnabled();
  });

  it("closes, and a second click on the button toggles it", async () => {
    const { user } = setup();
    await openBrowser(user);
    await user.click(within(browser()).getByRole("button", { name: "閉じる" }));
    expect(screen.queryByRole("region", { name: "フォルダを選択" })).not.toBeInTheDocument();
    await user.click(addButton());
    expect(browser()).toBeInTheDocument();
    await user.click(addButton());
    expect(screen.queryByRole("region", { name: "フォルダを選択" })).not.toBeInTheDocument();
  });

  it("is used in the app too when the project goes to a remote host, whose folders the dialog cannot reach", async () => {
    nativeDialog(true);
    const { t, user, onOpenWork } = await withRemoteOnScreen();
    const far = t.fars.get("sid-1");
    await user.click(addButton());
    expect(picker.pickLocalFolder).not.toHaveBeenCalled();
    expect(picker.canPickLocalFolder).toHaveBeenLastCalledWith(true);
    expect(screen.getByText("追加先: host-sid-1")).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "host-sid-1 上のフォルダを選択" })).toBeInTheDocument();
    await waitFor(() => expect(current()).toHaveTextContent("/home/u"));
    expect(far?.transport.callsOf("list_directory")).toHaveLength(1);
    expect(t.localTransport.callsOf("list_directory")).toEqual([]);

    await user.click(within(browser()).getByRole("button", { name: "このフォルダを開く" }));
    await waitFor(() => expect(far?.transport.callsOf("open_project").at(-1)?.path).toBe("/home/u"));
    expect(t.localTransport.callsOf("open_project")).toEqual([]);
    await waitFor(() => expect(onOpenWork).toHaveBeenCalled());
  });
});

describe("the projects of every host", () => {
  it("lists a remote host's projects after this PC's, each with the remote mark and the host's name", async () => {
    const { user, onOpenWork, t } = await mountHosts();
    const list = await projectList();
    await waitFor(() => expect(within(list).getAllByRole("button")).toHaveLength(4));
    const rows = within(list).getAllByRole("button");
    expect(rows.map((r) => r.querySelector(".dash-project-name")?.textContent)).toEqual(["repo", "beta", "repohost-sid-1", "server-onlyhost-sid-1"]);
    expect(within(rows[3]).getByRole("img", { name: /^リモート/ })).toHaveAttribute("title", "host-sid-1 · 接続中");
    expect(within(rows[0]).queryByRole("img", { name: /^リモート/ })).not.toBeInTheDocument();

    await user.click(rows[3]);
    expect(t.registry.getState().active).toBe("sid-1");
    expect(t.fars.get("sid-1")?.transport.callsOf("open_project").map((c) => c.path)).toEqual([FAR_ONLY.path]);
    expect(onOpenWork).toHaveBeenCalledTimes(1);
  });

  it("does not open a project of a host that is not connected", async () => {
    const { user, onOpenWork, t } = await mountHosts();
    await waitFor(() => expect(screen.getAllByRole("button", { name: /を開く/ })).toHaveLength(4));
    (t.api as FakeHostsApi).notice({
      type: "status",
      server_id: "sid-1",
      status: { connection: { state: "waiting_to_reconnect", attempt: 1, delay_ms: 1000, reason: "refused" }, server: null },
    });
    const row = await screen.findByRole("button", { name: /^server-only を開く/ });
    await waitFor(() => expect(row).toHaveAttribute("aria-disabled", "true"));
    await user.click(row);
    expect(t.registry.getState().active).toBe(LOCAL_HOST);
    expect(onOpenWork).not.toHaveBeenCalled();
  });
});

describe("the usage statistics", () => {
  it("shows the totals of the period, then the charts and the breakdowns", async () => {
    setup();
    const totals = await cards();
    // 300 input, 600 output, 9000 cache read, 1200 cache creation: 11,100 tokens in 2 sessions.
    expect(totals.getByText("合計トークン").parentElement).toHaveTextContent("11.1K");
    expect(totals.getByText("入力").parentElement).toHaveTextContent("300");
    expect(totals.getByText("出力").parentElement).toHaveTextContent("600");
    expect(totals.getByText("キャッシュ読み込み").parentElement).toHaveTextContent("9K");
    expect(totals.getByText("セッション").parentElement).toHaveTextContent("2");
    expect(totals.getByText("合計トークン").nextElementSibling).toHaveAttribute("title", "11,100");
    expect(screen.getByRole("heading", { name: "利用統計" })).toBeInTheDocument();
    expect(await screen.findByText("test-host", { selector: ".dash-sub" })).toBeInTheDocument();
  });

  it("reads the last 30 days first, and again with the period that is chosen", async () => {
    const { user, transport } = setup();
    await cards();
    expect(transport.callsOf("get_usage_stats")[0]).toMatchObject({ days: 30, refresh: false });
    const group = screen.getByRole("radiogroup", { name: "期間" });
    expect(within(group).getAllByRole("radio").map((r) => [r.textContent, r.getAttribute("aria-checked")])).toEqual([
      ["7日", "false"],
      ["30日", "true"],
      ["90日", "false"],
    ]);
    expect(document.querySelectorAll(".chart-day")).toHaveLength(30);

    await user.click(within(group).getByRole("radio", { name: "7日" }));
    await waitFor(() => expect(document.querySelectorAll(".chart-day")).toHaveLength(7));
    expect(transport.callsOf("get_usage_stats").at(-1)).toMatchObject({ days: 7 });
    expect(within(group).getByRole("radio", { name: "7日" })).toHaveAttribute("aria-checked", "true");

    await user.click(within(group).getByRole("radio", { name: "90日" }));
    await waitFor(() => expect(document.querySelectorAll(".chart-day")).toHaveLength(90));
    expect(transport.callsOf("get_usage_stats").at(-1)).toMatchObject({ days: 90 });
  });

  it("draws a bar per day of usage, stacked by harness, with the harnesses named below", async () => {
    const report = cannedUsageStats(3, 0);
    const [first, ...rest] = report.days;
    const stacked: UsageStats = {
      ...report,
      days: [
        {
          ...first,
          tokens: { input: 600, output: 400, cache_read: 0, cache_creation: 0, total: 1000 },
          sessions: 2,
          harnesses: [
            { harness: "codex", tokens: 400, sessions: 1 },
            { harness: "claude-code", tokens: 600, sessions: 1 },
          ],
        },
        ...rest,
      ],
      by_harness: [
        { name: "claude-code", tokens: { input: 0, output: 0, cache_read: 0, cache_creation: 0, total: 600 }, sessions: 1, other: false },
        { name: "codex", tokens: { input: 0, output: 0, cache_read: 0, cache_creation: 0, total: 400 }, sessions: 1, other: false },
      ],
      totals: { tokens: { input: 600, output: 400, cache_read: 0, cache_creation: 0, total: 1000 }, sessions: 2 },
      sources: [
        { harness: "claude-code", found: true, files: 1 },
        { harness: "codex", found: true, files: 1 },
        { harness: "opencode", found: false, files: 0 },
      ],
    };
    // Only the first day has usage in the stack: the other two (idle) have none.
    stacked.days = stacked.days.map((d, i) => (i === 0 ? d : { ...d, tokens: { input: 0, output: 0, cache_read: 0, cache_creation: 0, total: 0 }, sessions: 0, harnesses: [] }));
    setup({ usageStats: stacked });
    await cards();
    const days = [...document.querySelectorAll(".chart-day")];
    expect(days.map((d) => d.getAttribute("data-date"))).toEqual(stacked.days.map((d) => d.date));
    const bars = [...days[0].querySelectorAll("rect.chart-bar")];
    // Claude Code first (the order the core looks for harnesses), then Codex, stacked.
    expect(bars.map((b) => b.getAttribute("data-harness"))).toEqual(["claude-code", "codex"]);
    expect(bars.map((b) => b.getAttribute("class"))).toEqual(["chart-bar series-0", "chart-bar series-1"]);
    const [claude, codex] = bars.map((b) => [Number(b.getAttribute("y")), Number(b.getAttribute("height"))]);
    expect(claude[1] / codex[1]).toBeCloseTo(600 / 400);
    expect(codex[0] + codex[1]).toBeCloseTo(claude[0]); // the second sits on the first
    expect(claude[0] + claude[1]).toBeCloseTo(120); // the first sits on the baseline
    expect(days[1].querySelectorAll("rect.chart-bar")).toHaveLength(0);
    expect(days[0].querySelector("title")).toHaveTextContent("1,000 トークン");
    const legend = within(screen.getByRole("list", { name: "ハーネス" }));
    expect(legend.getAllByRole("listitem").map((li) => li.textContent)).toEqual(["Claude Code", "Codex"]);
    expect(screen.getByRole("img", { name: /日別のトークン数 \(3 日間/ })).toBeInTheDocument();
  });

  it("breaks the period down by model, harness and project, with shares and sessions", async () => {
    setup();
    await cards();
    for (const [title, name] of [["モデル別", "claude-opus-5-5"], ["ハーネス別", "Claude Code"], ["プロジェクト別", "repo"]] as const) {
      const section = within(screen.getByRole("region", { name: title }));
      const row = section.getByText(name).closest("li") as HTMLElement;
      expect(row).toHaveTextContent("11.1K");
      expect(row).toHaveTextContent("100%");
      expect(row).toHaveTextContent("2 セッション");
    }
    expect(screen.getByRole("region", { name: "ハーネス別" }).querySelector(".breakdown-fill")).toHaveClass("series-0");
  });

  it("splits shares between entries and names the sum that was cut off", async () => {
    const report = cannedUsageStats(7, 0);
    const t = (total: number) => ({ input: total, output: 0, cache_read: 0, cache_creation: 0, total });
    setup({
      usageStats: {
        ...report,
        by_project: [
          { name: "big", project: "big", tokens: t(750), sessions: 3, other: false },
          { name: "other", project: null, tokens: t(250), sessions: 1, other: true },
        ],
      },
    });
    await cards();
    const section = screen.getByRole("region", { name: "プロジェクト別" });
    const row = (li: Element) => [".breakdown-name", ".breakdown-value", ".breakdown-share", ".breakdown-sessions"].map((q) => li.querySelector(q)?.textContent);
    expect([...section.querySelectorAll("li")].map(row)).toEqual([
      ["big", "750", "75%", "3 セッション"],
      ["その他", "250", "25%", "1 セッション"],
    ]);
    expect(section.querySelector("li.other")).toHaveTextContent("その他");
  });

  it("lays the days out as a heatmap whose cells are as dark as the day's tokens", async () => {
    setup();
    await cards();
    const cells = [...document.querySelectorAll(".heatmap .heat-cell")] as HTMLElement[];
    expect(cells).toHaveLength(30);
    // The canned report has usage on the last two days only: 3,700 and 7,400 tokens.
    expect(cells.slice(0, 28).every((c) => c.getAttribute("data-level") === "0")).toBe(true);
    expect(cells.slice(28).map((c) => c.getAttribute("data-level"))).toEqual(["2", "4"]);
    expect(cells[29].getAttribute("title")).toContain("2026-10-20: 7,400 トークン");
    expect(screen.getByRole("img", { name: /ヒートマップ。最も多い日は 2026-10-20/ })).toBeInTheDocument();
  });

  it("says the logs were not found", async () => {
    const report = cannedUsageStats(30, 0);
    setup({
      usageStats: {
        ...report,
        sources: report.sources.map((s) => ({ ...s, found: false, files: 0 })),
        totals: { tokens: { input: 0, output: 0, cache_read: 0, cache_creation: 0, total: 0 }, sessions: 0 },
      },
    });
    expect(await screen.findByTestId("usage-no-logs")).toHaveTextContent("エージェントのログが見つかりません。");
    expect(screen.getByText(/Claude Code・Codex・OpenCode のセッションログ/)).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "期間の合計" })).not.toBeInTheDocument();
  });

  it("says nothing was used in a period that the logs cover", async () => {
    const report = cannedUsageStats(30, 0);
    setup({
      usageStats: {
        ...report,
        days: report.days.map((d) => ({ ...d, tokens: { input: 0, output: 0, cache_read: 0, cache_creation: 0, total: 0 }, sessions: 0, harnesses: [] })),
        totals: { tokens: { input: 0, output: 0, cache_read: 0, cache_creation: 0, total: 0 }, sessions: 0 },
      },
    });
    expect(await screen.findByTestId("usage-none")).toHaveTextContent("この 30 日間の利用はありません。");
  });

  it("shows that it is reading, and waits for the core to be connected", async () => {
    const { transport } = setup({ connected: false });
    expect(screen.getByRole("status")).toHaveTextContent("読み込み中…");
    expect(transport.callsOf("get_usage_stats")).toEqual([]);
    expect(screen.getByRole("button", { name: "再読み込み" })).toBeDisabled();
    act(() => transport.setStatus({ state: "open" }));
    await cards();
    expect(transport.callsOf("get_usage_stats")).toHaveLength(1);
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("shows the reason when the logs cannot be read, and reads again on request", async () => {
    const { user, core, transport } = setup();
    core.failures.set("get_usage_stats", new CommandError("internal", "cannot read the session logs"));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("利用統計を読めませんでした。");
    expect(alert).toHaveTextContent("cannot read the session logs");
    core.failures.clear();
    await user.click(within(alert).getByRole("button", { name: "再試行" }));
    await cards();
    expect(transport.callsOf("get_usage_stats").at(-1)).toMatchObject({ refresh: true });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("reads the logs again from the refresh button", async () => {
    const { user, transport } = setup();
    await cards();
    await user.click(screen.getByRole("button", { name: "再読み込み" }));
    await waitFor(() => expect(transport.callsOf("get_usage_stats")).toHaveLength(2));
    expect(transport.callsOf("get_usage_stats")[1]).toMatchObject({ refresh: true });
  });

  it("keeps the report on screen, dimmed, while the next one is read", async () => {
    const { user, core } = setup();
    await cards();
    core.before = (cmd) => (cmd.type === "get_usage_stats" ? new Promise<void>(() => {}) : undefined);
    await user.click(screen.getByRole("radio", { name: "7日" }));
    await waitFor(() => expect(screen.getByRole("region", { name: "利用統計" })).toHaveAttribute("aria-busy", "true"));
    expect(screen.getByRole("list", { name: "期間の合計" })).toBeInTheDocument();
    expect(document.querySelector(".usage-report")).toHaveClass("is-loading");
  });

  it("reads the statistics of the host on screen", async () => {
    const { t, user, onOpenWork } = await mountHosts();
    await cards();
    expect(t.localTransport.callsOf("get_usage_stats")).toHaveLength(1);
    expect(t.fars.get("sid-1")?.transport.callsOf("get_usage_stats") ?? []).toEqual([]);
    await user.click(await screen.findByRole("button", { name: /^server-only を開く/ }));
    expect(onOpenWork).toHaveBeenCalled();
    await waitFor(() => expect(t.fars.get("sid-1")?.transport.callsOf("get_usage_stats")).toHaveLength(1));
    expect(await screen.findByText("host-sid-1", { selector: ".dash-sub" })).toBeInTheDocument();
  });
});

// ---- hosts -----------------------------------------------------------------------------------

async function mountHosts() {
  const t = newTestHosts();
  (t.api as FakeHostsApi).hosts = [hostInfo("sid-1")];
  const onOpenWork = vi.fn();
  render(
    <HostsProvider registry={t.registry}>
      <Dashboard onOpenWork={onOpenWork} />
    </HostsProvider>,
  );
  await act(() => t.registry.start());
  await waitFor(() => expect(t.registry.getState().projects.length).toBeGreaterThanOrEqual(4));
  return { t, onOpenWork, user: userEvent.setup() };
}

/** The dashboard with a remote host's project on screen (the host is the active one). */
async function withRemoteOnScreen() {
  const { t, onOpenWork, user } = await mountHosts();
  await act(() => t.registry.selectProject("sid-1", FAR_ONLY));
  await waitFor(() => expect(t.registry.getState().active).toBe("sid-1"));
  return { t, onOpenWork, user };
}
