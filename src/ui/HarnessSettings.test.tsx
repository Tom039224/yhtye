// The settings modal's "ハーネス" section: whether each harness is installed,
// where its commands were found, manual paths of the main executables, and
// the detection again; the agent settings follow what it finds.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../App";
import { CommandError } from "../api/transport";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";

const OPENCODE_BIN = "/opt/tools/opencode";

async function setup(configure?: (core: FakeCore) => void) {
  const transport = new MemoryTransport();
  const core = new FakeCore(PROJECT, FULL_RUN.start);
  core.attach(transport);
  configure?.(core);
  const store = new AppStore(transport);
  store.start();
  render(
    <StoreContext.Provider value={store}>
      <App />
    </StoreContext.Provider>,
  );
  await act(() => store.openProject(PROJECT.path));
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "設定" }));
  const dialog = await screen.findByRole("dialog", { name: "設定" });
  return { user, dialog, core, transport };
}

async function openHarnesses(configure?: (core: FakeCore) => void) {
  const s = await setup(configure);
  await s.user.click(within(s.dialog).getByRole("button", { name: "ハーネス" }));
  await within(s.dialog).findByRole("list", { name: "ハーネス" });
  return s;
}

function row(dialog: HTMLElement, label: string): HTMLElement {
  return within(within(dialog).getByRole("list", { name: "ハーネス" })).getByRole("listitem", { name: label });
}

/** The lines of a harness's required commands. */
function requirements(r: HTMLElement): string[] {
  const list = within(r).getByRole("list", { name: /必要なコマンド/ });
  return within(list)
    .getAllByRole("listitem")
    .map((li) => li.textContent ?? "");
}

describe("harness settings", () => {
  it("shows each harness's state, paths and where they were found", async () => {
    const { dialog, transport } = await openHarnesses((c) => {
      c.harnesses.found.devin = { path: "/home/u/.local/bin/devin", source: "known_dir" };
    });
    expect(transport.callsOf("get_harnesses")).toHaveLength(1);
    expect(within(dialog).queryByRole("group", { name: "設定の範囲" })).toBeNull();

    const claude = row(dialog, "Claude Code");
    expect(within(claude).getByText("インストール済み")).toBeTruthy();
    expect(requirements(claude)).toEqual(["npx/usr/bin/npxPATH"]);

    const devin = row(dialog, "Devin");
    expect(within(devin).getByText("インストール済み")).toBeTruthy();
    expect(requirements(devin)).toEqual(["devin/home/u/.local/bin/devin既知の場所"]);

    const opencode = row(dialog, "OpenCode");
    expect(within(opencode).getByText("未インストール")).toBeTruthy();
    expect(requirements(opencode)).toEqual(["opencode見つかりません"]);

    // Codex's npx is found but only automatically; codex itself is missing.
    const codex = row(dialog, "Codex");
    expect(within(codex).getByText("未インストール")).toBeTruthy();
    expect(requirements(codex)).toEqual(["codex見つかりません", "npx/usr/bin/npx自動検出のみ"]);
    expect(within(codex).getByRole("textbox", { name: "Codex の codex のパス" })).toBeTruthy();
    expect(within(codex).getByRole("button", { name: "自動検出に戻す" })).toHaveProperty("disabled", true);
  });

  it("shows Google Antigravity found by the name of its Python archive", async () => {
    const { dialog } = await openHarnesses((c) => {
      c.harnesses.found["agy_acp_server.par"] = {
        path: "/home/u/.local/share/agy-acp-server/agy_acp_server.par",
        source: "known_dir",
      };
    });
    const agy = row(dialog, "Google Antigravity");
    expect(within(agy).getByText("インストール済み")).toBeTruthy();
    expect(requirements(agy)).toEqual([
      "agy_acp_server.par/home/u/.local/share/agy-acp-server/agy_acp_server.par既知の場所",
    ]);
    expect(within(agy).getByRole("textbox", { name: "Google Antigravity の agy_acp_server.par のパス" })).toBeTruthy();
  });

  it("shows Grok Build found in its own install directory", async () => {
    const { dialog } = await openHarnesses((c) => {
      c.harnesses.found.grok = { path: "/home/u/.grok/bin/grok", source: "known_dir" };
    });
    const grok = row(dialog, "Grok Build");
    expect(within(grok).getByText("インストール済み")).toBeTruthy();
    expect(requirements(grok)).toEqual(["grok/home/u/.grok/bin/grok既知の場所"]);
    expect(within(grok).getByRole("textbox", { name: "Grok Build の grok のパス" })).toBeTruthy();
  });

  it("saves a manual path, then goes back to the automatic search", async () => {
    const { user, dialog, core } = await openHarnesses((c) => c.harnesses.executables.add(OPENCODE_BIN));
    const opencode = row(dialog, "OpenCode");
    await user.type(within(opencode).getByRole("textbox", { name: "OpenCode の opencode のパス" }), `  ${OPENCODE_BIN} `);
    await user.click(within(opencode).getByRole("button", { name: "保存" }));

    await waitFor(() => expect(within(row(dialog, "OpenCode")).getByText("インストール済み")).toBeTruthy());
    expect(core.harnesses.overrides).toEqual({ opencode: OPENCODE_BIN });
    expect(requirements(row(dialog, "OpenCode"))).toEqual([`opencode${OPENCODE_BIN}手動`]);
    const box = within(row(dialog, "OpenCode")).getByRole("textbox", { name: "OpenCode の opencode のパス" });
    expect((box as HTMLInputElement).value).toBe(OPENCODE_BIN);

    await user.click(within(row(dialog, "OpenCode")).getByRole("button", { name: "自動検出に戻す" }));
    await waitFor(() => expect(within(row(dialog, "OpenCode")).getByText("未インストール")).toBeTruthy());
    expect(core.harnesses.overrides).toEqual({});
    expect((within(row(dialog, "OpenCode")).getByRole("textbox") as HTMLInputElement).value).toBe("");
  });

  it("shows a rejected path on its row and keeps what was typed", async () => {
    const { user, dialog, core } = await openHarnesses();
    const codex = row(dialog, "Codex");
    await user.type(within(codex).getByRole("textbox"), "bin/codex");
    await user.click(within(codex).getByRole("button", { name: "保存" }));
    expect((await within(codex).findByRole("alert")).textContent).toContain("not an absolute path");
    expect((within(codex).getByRole("textbox") as HTMLInputElement).value).toBe("bin/codex");
    expect(core.harnesses.overrides).toEqual({});
    expect(within(row(dialog, "OpenCode")).queryByRole("alert")).toBeNull();

    await user.clear(within(codex).getByRole("textbox"));
    await user.click(within(codex).getByRole("button", { name: "保存" }));
    expect((await within(codex).findByRole("alert")).textContent).toContain("パスを入力してください");
  });

  it("shows a stored manual path that cannot be used", async () => {
    const { dialog } = await openHarnesses((c) => (c.harnesses.overrides = { devin: "/gone/devin" }));
    const devin = row(dialog, "Devin");
    expect(within(devin).getByText("未インストール")).toBeTruthy();
    expect(within(devin).getByRole("alert").textContent).toContain("手動パス /gone/devin を使えません");
    expect(within(devin).getByRole("button", { name: "自動検出に戻す" })).toHaveProperty("disabled", false);
  });

  it("detects again with 再検出", async () => {
    const { user, dialog, core, transport } = await openHarnesses();
    expect(within(row(dialog, "OpenCode")).getByText("未インストール")).toBeTruthy();
    core.harnesses.found.opencode = { path: "/usr/local/bin/opencode", source: "known_dir" };
    await user.click(within(dialog).getByRole("button", { name: "再検出" }));
    await waitFor(() => expect(within(row(dialog, "OpenCode")).getByText("インストール済み")).toBeTruthy());
    expect(transport.callsOf("detect_harnesses")).toHaveLength(1);
  });

  it("shows a failed detection", async () => {
    const { user, dialog } = await setup((c) => c.failures.set("get_harnesses", new CommandError("unavailable", "boom")));
    await user.click(within(dialog).getByRole("button", { name: "ハーネス" }));
    expect((await within(dialog).findByRole("alert")).textContent).toContain("boom");
  });

  it("retries with 再検出 after the first look failed, and fills in the stored manual paths", async () => {
    const { user, dialog, transport } = await setup((c) => {
      c.failures.set("get_harnesses", new CommandError("unavailable", "boom"));
      c.harnesses.executables.add(OPENCODE_BIN);
      c.harnesses.overrides = { opencode: OPENCODE_BIN };
    });
    await user.click(within(dialog).getByRole("button", { name: "ハーネス" }));
    expect((await within(dialog).findByRole("alert")).textContent).toContain("boom");
    expect(within(dialog).queryByRole("list", { name: "ハーネス" })).toBeNull();

    const detect = within(dialog).getByRole("button", { name: "再検出" });
    expect(detect).toHaveProperty("disabled", false);
    await user.click(detect);

    await within(dialog).findByRole("list", { name: "ハーネス" });
    expect(transport.callsOf("detect_harnesses")).toHaveLength(1);
    expect(within(dialog).queryByRole("alert")).toBeNull();
    const opencode = row(dialog, "OpenCode");
    expect(within(opencode).getByText("インストール済み")).toBeTruthy();
    expect((within(opencode).getByRole("textbox") as HTMLInputElement).value).toBe(OPENCODE_BIN);
    expect(within(opencode).getByRole("button", { name: "自動検出に戻す" })).toHaveProperty("disabled", false);
  });

  it("keeps what was typed when detecting again after a successful first look", async () => {
    const { user, dialog } = await openHarnesses();
    await user.type(within(row(dialog, "Codex")).getByRole("textbox"), "/opt/codex");
    await user.click(within(dialog).getByRole("button", { name: "再検出" }));
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "再検出" })).toHaveProperty("disabled", false));
    expect((within(row(dialog, "Codex")).getByRole("textbox") as HTMLInputElement).value).toBe("/opt/codex");
  });

  it("the agent settings offer a harness found in the harness tab", async () => {
    const { user, dialog } = await setup((c) => {
      c.agents.harnesses = [];
      c.harnesses.executables.add(OPENCODE_BIN);
    });
    expect(await within(dialog).findByText(/設定 › ハーネス を開いて/)).toBeTruthy();

    await user.click(within(dialog).getByRole("button", { name: "ハーネス" }));
    await within(dialog).findByRole("list", { name: "ハーネス" });
    await user.type(within(row(dialog, "OpenCode")).getByRole("textbox"), OPENCODE_BIN);
    await user.click(within(row(dialog, "OpenCode")).getByRole("button", { name: "保存" }));
    await waitFor(() => expect(within(row(dialog, "OpenCode")).getByText("インストール済み")).toBeTruthy());

    await user.click(within(dialog).getByRole("button", { name: "エージェント" }));
    expect(await within(dialog).findByText(/OpenCode: モデル一覧を取得できません/)).toBeTruthy();
    expect(within(dialog).queryByText(/設定 › ハーネス を開いて/)).toBeNull();
    expect(within(dialog).getByRole("group", { name: "設定の範囲" })).toBeTruthy();
  });
});
