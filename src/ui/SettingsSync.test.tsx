// The settings screens with other clients on the same core (T-65): settings
// changed elsewhere are read again (or, while the user is editing, a banner
// offers it), and a save from a screen that shows old agent settings is refused
// by the core (`conflict`) and the screen reads them again.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../App";
import type { ApiEventBody, RoleSettings } from "../api/generated";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { ev } from "../test/events";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";

const SONNET: RoleSettings = {
  candidates: [{ harness: "claude-code", model: "sonnet", effort: null, note: "" }],
  default: { harness: "claude-code", model: "sonnet", effort: null },
};

async function setup(section: string) {
  const transport = new MemoryTransport();
  const core = new FakeCore(PROJECT, FULL_RUN.start);
  core.attach(transport);
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
  await user.click(within(dialog).getByRole("button", { name: section }));
  return { transport, core, user, dialog };
}

/** What another client's change looks like to this one. */
function changed(transport: MemoryTransport, body: Extract<ApiEventBody, { type: "settings_changed" }>) {
  act(() => transport.emit(ev(0, body, true, "")));
}

describe("agent settings changed elsewhere", () => {
  it("are read again when announced, but not for the revision already shown", async () => {
    const { transport, core, dialog } = await setup("エージェント");
    const model = () => within(within(dialog).getByRole("group", { name: "実装" })).getByRole("combobox", { name: "実装 候補1 のモデル" });
    await waitFor(() => expect(model()).toHaveDisplayValue("Haiku (haiku)"));
    const reads = transport.callsOf("get_agent_settings").length;
    changed(transport, { type: "settings_changed", kind: "agent_settings", revision: core.agents.revision });
    expect(transport.callsOf("get_agent_settings")).toHaveLength(reads);

    core.agents.set(PROJECT.id, "implementer", SONNET); // another client's save
    changed(transport, { type: "settings_changed", kind: "agent_settings", project: PROJECT.id, revision: core.agents.revision });
    await waitFor(() => expect(model()).toHaveDisplayValue("Sonnet (sonnet)"));
    expect(screen.queryByTestId("settings-stale")).toBeNull();
  });

  it("show a banner instead while a note is being typed, and read them again on request", async () => {
    const { transport, core, user, dialog } = await setup("エージェント");
    const group = () => within(dialog).getByRole("group", { name: "実装" });
    await waitFor(() => expect(within(group()).getByRole("checkbox", { name: "全体の設定を使う" })).toBeEnabled());
    await user.click(within(group()).getByRole("checkbox", { name: "全体の設定を使う" }));
    const note = () => within(group()).getByRole("textbox", { name: "実装 候補1 の用途メモ" });
    await waitFor(() => expect(note()).toBeEnabled());
    await user.click(note());
    await user.keyboard("half");
    const reads = transport.callsOf("get_agent_settings").length;
    core.agents.set(null, "reviewer", SONNET);
    changed(transport, { type: "settings_changed", kind: "agent_settings", revision: core.agents.revision });
    expect(await screen.findByTestId("settings-stale")).toHaveTextContent("他の端末で設定が変更されました");
    expect(transport.callsOf("get_agent_settings")).toHaveLength(reads);
    expect(note()).toHaveValue("half");
    // Leaving the note saves it from the old screen: refused, nothing is overwritten.
    await user.click(within(screen.getByTestId("settings-stale")).getByRole("button", { name: "再読込" }));
    await waitFor(() => expect(transport.callsOf("get_agent_settings").length).toBeGreaterThan(reads));
    expect(screen.queryByTestId("settings-stale")).toBeNull();
    expect(core.agents.global.reviewer).toEqual(SONNET);
  });

  it("are not overwritten by a save from the old screen: it is refused and the screen reads them again", async () => {
    const { transport, core, user, dialog } = await setup("エージェント");
    const group = () => within(dialog).getByRole("group", { name: "実装" });
    await waitFor(() => expect(within(group()).getByRole("checkbox", { name: "全体の設定を使う" })).toBeEnabled());
    const shown = core.agents.revision;
    core.agents.set(PROJECT.id, "implementer", SONNET); // another client's save, not announced yet
    await user.click(within(group()).getByRole("checkbox", { name: "全体の設定を使う" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("他の端末で設定が変更されていたため");
    expect(transport.callsOf("set_agent_settings").at(-1)?.expected_revision).toBe(shown);
    // The other client's settings stay, and are on screen now.
    expect(core.agents.projects.get(PROJECT.id)?.implementer).toEqual(SONNET);
    await waitFor(() => expect(within(group()).getByRole("combobox", { name: "実装 候補1 のモデル" })).toHaveDisplayValue("Sonnet (sonnet)"));
  });
});

describe("secrets and harnesses changed elsewhere", () => {
  it("list the secret names again, or show a banner while the form is filled in", async () => {
    const { transport, core, user, dialog } = await setup("秘密の環境変数");
    await waitFor(() => expect(within(dialog).getByText("登録されている変数はありません。")).toBeInTheDocument());
    core.secretNames = ["FROM_PHONE"];
    changed(transport, { type: "settings_changed", kind: "secrets" });
    expect(await within(dialog).findByText("FROM_PHONE")).toBeInTheDocument();

    await user.type(within(dialog).getByRole("textbox", { name: "変数名" }), "MINE");
    core.secretNames = ["FROM_PHONE", "OTHER"];
    changed(transport, { type: "settings_changed", kind: "secrets" });
    expect(await screen.findByTestId("settings-stale")).toBeInTheDocument();
    expect(within(dialog).queryByText("OTHER")).toBeNull();
    await user.click(within(screen.getByTestId("settings-stale")).getByRole("button", { name: "再読込" }));
    expect(await within(dialog).findByText("OTHER")).toBeInTheDocument();
    expect(within(dialog).getByRole("textbox", { name: "変数名" })).toHaveValue("MINE");
  });

  it("detect the harnesses again when another client set a path", async () => {
    const { transport, core, dialog } = await setup("ハーネス");
    const box = () => within(dialog).getByRole("textbox", { name: "OpenCode の opencode のパス" });
    await waitFor(() => expect(box()).toHaveValue(""));
    core.harnesses.executables.add("/opt/bin/opencode");
    core.harnesses.setPath("opencode", "/opt/bin/opencode");
    const reads = transport.callsOf("get_harnesses").length;
    changed(transport, { type: "settings_changed", kind: "harnesses" });
    await waitFor(() => expect(box()).toHaveValue("/opt/bin/opencode"));
    expect(transport.callsOf("get_harnesses")).toHaveLength(reads + 1);
    // Other kinds are not this section's.
    changed(transport, { type: "settings_changed", kind: "secrets" });
    expect(transport.callsOf("get_harnesses")).toHaveLength(reads + 1);
  });
});
