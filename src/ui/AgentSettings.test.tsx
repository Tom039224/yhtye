// The settings modal's "エージェント" section (Stage 7b, tables in 7d): per role a
// table of candidate rows (harness × model × effort + a note, one default), the
// global and project scopes, inheritance, and the edits it saves.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../App";
import type { AgentChoice, Candidate, EffortOption, HarnessInfo, HarnessModels, RoleSettings } from "../api/generated";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { DUPLICATE_ROW, MAX_MATCHES, addRow, newRow, pickerModels, removeRow, updateRow, withDefaultRow } from "./agentSettings";

const HAIKU: AgentChoice = { harness: "claude-code", model: "haiku", effort: null };
const SONNET_HIGH: AgentChoice = { harness: "claude-code", model: "sonnet", effort: "high" };
const FREE = "opencode/muse-spark-1.3-contributor-free";
const OPENCODE: HarnessInfo = { id: "opencode", label: "OpenCode", requires_model: true };
const PLAIN = { requires_model: false };

const row = (c: AgentChoice, note = ""): Candidate => ({ ...c, note });
const efforts = (...values: string[]): EffortOption[] => values.map((v) => ({ value: v, name: v, description: null }));

/** OpenCode's long model list in miniature: 3 providers × 10 models (> LONG_LIST), efforts not listed. */
function opencodeModels(): HarnessModels {
  const models = ["opencode", "openai", "ollama"].flatMap((p) =>
    Array.from({ length: 10 }, (_, i) => ({ value: `${p}/model-${i}`, name: `${p} model ${i}`, description: null, efforts: null })),
  );
  models[0] = { value: FREE, name: "Muse Spark 1.3 (free)", description: null, efforts: null };
  return { harness: "opencode", models, current: "openai/model-1", fetched_at_ms: 0 };
}

function withOpenCode(core: FakeCore) {
  core.agents.harnesses.push(OPENCODE);
  core.agents.models.opencode = opencodeModels();
  core.agents.efforts[`opencode/${FREE}`] = efforts("low", "high");
}

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
  return { transport, core };
}

async function openSettings() {
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "設定" }));
  const dialog = await screen.findByRole("dialog", { name: "設定" });
  return { user, dialog };
}

function role(dialog: HTMLElement, name: string): HTMLElement {
  return within(dialog).getByRole("group", { name });
}

/** The rows of a role's table (without the header). */
function rowsOf(group: HTMLElement): HTMLElement[] {
  return within(within(group).getByRole("table")).getAllByRole("row").slice(1);
}

describe("settings modal", () => {
  it("opens from the bottom of the icon strip, and closes with Esc and ×", async () => {
    await setup();
    const rail = screen.getByRole("navigation", { name: "views" });
    const buttons = within(rail).getAllByRole("button");
    expect(buttons.at(-1)).toHaveAccessibleName("設定");
    expect(screen.queryByRole("button", { name: "エージェントの設定" })).toBeNull();

    const { user, dialog } = await openSettings();
    expect(within(dialog).getByRole("button", { name: "エージェント" })).toHaveAttribute("aria-current", "page");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "設定" })).toBeNull();
    const again = await openSettings();
    await again.user.click(within(again.dialog).getByRole("button", { name: "閉じる" }));
    expect(screen.queryByRole("dialog", { name: "設定" })).toBeNull();
  });

  it("shows every role's table with the built-in row, for this project by default", async () => {
    const { transport } = await setup();
    const { dialog } = await openSettings();
    await waitFor(() => expect(rowsOf(role(dialog, "実装"))).toHaveLength(1));
    for (const name of ["オーケストレータ", "実装", "調査", "レビュー"]) {
      const group = role(dialog, name);
      expect(within(group).getByRole("checkbox", { name: "全体の設定を使う" })).toBeChecked();
      expect(within(group).getByRole("combobox", { name: `${name} 候補1 のモデル` })).toHaveDisplayValue("Haiku (haiku)");
      expect(within(group).getByRole("radio", { name: `${name} 候補1 を既定にする` })).toBeChecked();
      expect(within(group).getByRole("combobox", { name: `${name} 候補1 の effort` })).toBeDisabled();
    }
    expect(within(dialog).getByRole("button", { name: /このプロジェクト/ })).toHaveAttribute("aria-pressed", "true");
    expect(transport.callsOf("get_agent_settings")[0].project).toBe(PROJECT.id);
    expect(transport.callsOf("list_harness_models")).toEqual([{ type: "list_harness_models", harness: "claude-code", refresh: false }]);
    expect(dialog).toHaveTextContent("新しく起動するエージェントから有効");
  });

  it("adds rows with the same model and different efforts, a note, and moves the default", async () => {
    const { transport, core } = await setup();
    const { user, dialog } = await openSettings();
    const implementer = () => role(dialog, "実装");
    await waitFor(() => expect(rowsOf(implementer())).toHaveLength(1));
    // Inherited settings cannot be edited until the role stops inheriting.
    expect(within(implementer()).getByRole("button", { name: "+ 行を追加" })).toBeDisabled();

    await user.click(within(implementer()).getByRole("checkbox", { name: "全体の設定を使う" }));
    await waitFor(() => expect(within(implementer()).getByRole("button", { name: "+ 行を追加" })).toBeEnabled());
    await user.click(within(implementer()).getByRole("button", { name: "+ 行を追加" }));
    await waitFor(() => expect(rowsOf(implementer())).toHaveLength(2));
    // The new row is the first model nobody uses yet; switch it to Sonnet.
    await user.selectOptions(within(implementer()).getByRole("combobox", { name: "実装 候補2 のモデル" }), "sonnet");
    const effort = () => within(implementer()).getByRole("combobox", { name: "実装 候補2 の effort" });
    await waitFor(() => expect(effort()).toBeEnabled());
    expect(within(effort()).getAllByRole("option").map((o) => o.textContent)).toEqual(["指定なし", "low", "medium", "high"]);
    await user.selectOptions(effort(), "high");
    await user.type(within(implementer()).getByRole("textbox", { name: "実装 候補2 の用途メモ" }), "設計が絡む難しい変更");
    await user.tab();
    await user.click(within(implementer()).getByRole("radio", { name: "実装 候補2 を既定にする" }));
    // A second Sonnet row with another effort is a different row.
    await user.click(within(implementer()).getByRole("button", { name: "+ 行を追加" }));
    await waitFor(() => expect(rowsOf(implementer())).toHaveLength(3));

    const saved = core.agents.projects.get(PROJECT.id)?.implementer;
    expect(saved?.candidates.slice(0, 2)).toEqual([row(HAIKU), row(SONNET_HIGH, "設計が絡む難しい変更")]);
    expect(saved?.default).toEqual(SONNET_HIGH);
    expect(core.agents.global.implementer).toBeNull();
    expect(transport.callsOf("set_agent_settings").every((c) => c.project === PROJECT.id && c.role === "implementer")).toBe(true);
    // The only row of another role cannot be removed.
    expect(within(role(dialog, "レビュー")).getByRole("button", { name: "レビュー 候補1 を削除" })).toBeDisabled();
  });

  it("refuses two rows with the same harness × model × effort and removes rows", async () => {
    const { transport, core } = await setup((c) => {
      c.agents.global.implementer = { candidates: [row(HAIKU), row(SONNET_HIGH)], default: HAIKU };
    });
    const { user, dialog } = await openSettings();
    await user.click(within(dialog).getByRole("button", { name: "全体" }));
    const implementer = () => role(dialog, "実装");
    await waitFor(() => expect(rowsOf(implementer())).toHaveLength(2));
    const before = transport.callsOf("set_agent_settings").length;
    await user.selectOptions(within(implementer()).getByRole("combobox", { name: "実装 候補2 のモデル" }), "haiku");
    // Moving row 2 to haiku would make it (haiku, no effort): the effort is dropped -> same as row 1.
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(DUPLICATE_ROW);
    expect(transport.callsOf("set_agent_settings")).toHaveLength(before);

    await user.click(within(implementer()).getByRole("button", { name: "実装 候補1 を削除" }));
    await waitFor(() => expect(core.agents.global.implementer).toEqual({ candidates: [row(SONNET_HIGH)], default: SONNET_HIGH }));
  });

  it("edits the global layer in the global scope", async () => {
    const { transport, core } = await setup();
    const { user, dialog } = await openSettings();
    await user.click(within(dialog).getByRole("button", { name: "全体" }));
    await waitFor(() => expect(transport.callsOf("get_agent_settings").at(-1)?.project).toBeUndefined());
    const reviewer = () => role(dialog, "レビュー");
    await waitFor(() => expect(within(reviewer()).getByRole("checkbox", { name: "組み込みの既定を使う" })).toBeChecked());
    await user.click(within(reviewer()).getByRole("checkbox", { name: "組み込みの既定を使う" }));
    await waitFor(() => expect(core.agents.global.reviewer).toEqual({ candidates: [row(HAIKU)], default: HAIKU }));
    expect(transport.callsOf("set_agent_settings")[0].project).toBeUndefined();
    await user.click(within(reviewer()).getByRole("checkbox", { name: "組み込みの既定を使う" }));
    await waitFor(() => expect(core.agents.global.reviewer).toBeNull());
  });

  it("reports a harness whose models cannot be read and keeps its saved rows", async () => {
    await setup((core) => {
      core.agents.harnesses.push({ id: "other", label: "Other", ...PLAIN });
      core.agents.global.implementer = {
        candidates: [row(HAIKU), row({ harness: "other", model: null, effort: null })],
        default: HAIKU,
      };
    });
    const { dialog } = await openSettings();
    await waitFor(() => expect(dialog).toHaveTextContent("Other: モデル一覧を取得できません"));
    const implementer = role(dialog, "実装");
    expect(within(implementer).getByRole("combobox", { name: "実装 候補2 のハーネス" })).toHaveDisplayValue("Other");
    expect(rowsOf(implementer)[1]).toHaveTextContent("既定のモデル");
  });

  it("searches OpenCode's long model list in a picker grouped by provider and reads its efforts on demand", async () => {
    const { transport, core } = await setup(withOpenCode);
    const { user, dialog } = await openSettings();
    await user.click(within(dialog).getByRole("button", { name: "全体" }));
    const implementer = () => role(dialog, "実装");
    await waitFor(() => expect(transport.callsOf("list_harness_models").map((c) => c.harness)).toEqual(["claude-code", "opencode"]));
    await user.click(within(implementer()).getByRole("checkbox", { name: "組み込みの既定を使う" }));
    await waitFor(() => expect(core.agents.global.implementer).not.toBeNull());
    await user.click(within(implementer()).getByRole("button", { name: "+ 行を追加" }));
    await waitFor(() => expect(rowsOf(implementer())).toHaveLength(2));
    // Row 2: switch to OpenCode; its first (long-list) model has no effort list yet.
    await user.selectOptions(within(implementer()).getByRole("combobox", { name: "実装 候補2 のハーネス" }), "opencode");
    const picker = () => within(implementer()).getByRole("button", { name: "実装 候補2 のモデル" });
    await waitFor(() => expect(picker()).toBeEnabled());
    await user.click(picker());
    const list = within(implementer()).getByRole("listbox", { name: "実装 候補2 のモデルの候補" });
    expect(list).toHaveTextContent("検索語を入力してください");

    const search = within(implementer()).getByRole("searchbox", { name: "実装 候補2 のモデルを検索" });
    await user.type(search, "model 1");
    expect(within(list).getAllByRole("option")).toHaveLength(3);
    expect(list).toHaveTextContent("openai");
    expect(list).toHaveTextContent("ollama");
    // Esc closes the picker, not the dialog.
    await user.keyboard("{Escape}");
    expect(within(implementer()).queryByRole("listbox")).toBeNull();
    expect(screen.getByRole("dialog", { name: "設定" })).toBeInTheDocument();

    await user.click(picker());
    await user.type(within(implementer()).getByRole("searchbox"), "free");
    await user.click(within(implementer()).getByRole("option", { name: `Muse Spark 1.3 (free) (${FREE})` }));
    const effort = () => within(implementer()).getByRole("combobox", { name: "実装 候補2 の effort" });
    await waitFor(() => expect(effort()).toBeEnabled());
    expect(transport.callsOf("list_model_efforts")).toEqual([{ type: "list_model_efforts", harness: "opencode", model: FREE }]);
    expect(within(effort()).getAllByRole("option").map((o) => o.textContent)).toEqual(["指定なし", "low", "high"]);
    await user.selectOptions(effort(), "low");
    const free: AgentChoice = { harness: "opencode", model: FREE, effort: "low" };
    await waitFor(() => expect(core.agents.global.implementer?.candidates[1]).toEqual(row(free)));
  });

  it("shows no write-restriction warning: every harness's orchestrator can write (Stage 8d)", async () => {
    const free: AgentChoice = { harness: "opencode", model: FREE, effort: null };
    await setup((core) => {
      withOpenCode(core);
      core.agents.global.orchestrator = { candidates: [row(HAIKU), row(free)], default: HAIKU };
      core.agents.global.implementer = { candidates: [row(HAIKU), row(free)], default: HAIKU };
    });
    const { dialog } = await openSettings();
    const orchestrator = role(dialog, "オーケストレータ");
    await waitFor(() => expect(rowsOf(orchestrator)).toHaveLength(2));
    expect(within(orchestrator).queryByRole("note")).toBeNull();
    expect(within(dialog).queryByText(/書き込み制限/)).toBeNull();
    expect(within(orchestrator).queryByRole("option", { name: /OpenCode.*⚠/ })).toBeNull();
    expect(within(orchestrator).getAllByRole("option", { name: "OpenCode" }).length).toBeGreaterThan(0);
  });

  it("shows settings that name a harness which is no longer installed", async () => {
    await setup((core) => {
      core.agents.global.implementer = {
        candidates: [row(HAIKU), row({ harness: "opencode", model: FREE, effort: null })],
        default: { harness: "opencode", model: FREE, effort: null },
      };
    });
    const { dialog } = await openSettings();
    const implementer = role(dialog, "実装");
    await waitFor(() => expect(within(implementer).getByRole("alert")).toHaveTextContent("opencode が見つかりません"));
    expect(within(implementer).getByRole("alert")).toHaveTextContent("既定 (Claude Code · Haiku (haiku)) で起動します");
    expect(within(implementer).getByRole("combobox", { name: "実装 候補2 のハーネス" })).toHaveDisplayValue("opencode (見つからない)");
    expect(within(role(dialog, "レビュー")).queryByRole("alert")).toBeNull();
  });
});

describe("agent settings helpers", () => {
  const rows = (...c: AgentChoice[]): RoleSettings => ({ candidates: c.map((x) => row(x)), default: c[0] });
  const SONNET: AgentChoice = { harness: "claude-code", model: "sonnet", effort: null };

  it("keep the default on its row when the row changes, and refuse duplicates", () => {
    const s = rows(HAIKU, SONNET);
    const moved = updateRow(s, 0, { model: "sonnet", effort: "high" });
    expect(moved).toEqual({ ok: { candidates: [row(SONNET_HIGH), row(SONNET)], default: SONNET_HIGH } });
    expect(updateRow(s, 0, { model: "sonnet" })).toEqual({ error: DUPLICATE_ROW });
    // Only the note changed: never a duplicate of itself.
    expect(updateRow(s, 1, { note: "x" })).toEqual({ ok: { candidates: [row(HAIKU), { ...row(SONNET), note: "x" }], default: HAIKU } });
    expect(addRow(s, row(SONNET))).toEqual({ error: DUPLICATE_ROW });
    expect(addRow(s, row(SONNET_HIGH))).toEqual({ ok: { candidates: [...s.candidates, row(SONNET_HIGH)], default: HAIKU } });
  });

  it("keep at least one row and move the default off a removed one", () => {
    expect(removeRow(rows(HAIKU), 0)).toBeNull();
    const s = rows(HAIKU, SONNET_HIGH);
    expect(removeRow(s, 0)).toEqual({ candidates: [row(SONNET_HIGH)], default: SONNET_HIGH });
    expect(removeRow(s, 1)).toEqual({ candidates: [row(HAIKU)], default: HAIKU });
    expect(withDefaultRow(s, 1).default).toEqual(SONNET_HIGH);
  });

  it("propose a new row nobody has, starting with the last row's harness", () => {
    const harnesses = [
      { id: "a", label: "A", ...PLAIN },
      { id: "b", label: "B", ...PLAIN },
      { id: "c", label: "C", ...PLAIN, requires_model: true },
    ];
    const listed = (harness: string, ...values: string[]): HarnessModels => ({
      harness,
      models: values.map((value) => ({ value, name: value, description: null, efforts: [] })),
      current: null,
      fetched_at_ms: 0,
    });
    const state = (m: HarnessModels) => ({ models: m, error: null, loading: false });
    const models = { a: state(listed("a", "x", "y")), b: state(listed("b")), c: state(listed("c")) };
    const one = (h: string, m: string | null): RoleSettings => rows({ harness: h, model: m, effort: null });
    expect(newRow(harnesses, models, one("a", "x"))).toEqual({ harness: "a", model: "y", effort: null, note: "" });
    // a is exhausted: b has no model option (its default), c requires a model and has none.
    expect(newRow(harnesses, models, rows({ harness: "a", model: "x", effort: null }, { harness: "a", model: "y", effort: null }))).toEqual({
      harness: "b",
      model: null,
      effort: null,
      note: "",
    });
    expect(newRow(harnesses, { a: state(listed("a", "x")), c: state(listed("c")) }, one("a", "x"))).toBeNull();
    expect(newRow(harnesses, {}, one("a", "x"))).toBeNull();
  });

  it("limit search matches, group them by provider and show nothing for an empty search", () => {
    const listed = opencodeModels();
    expect(pickerModels(listed, "  ")).toEqual({ groups: [], more: 0 });
    const all = pickerModels(listed, "model");
    expect(all.groups.map((g) => g.provider)).toEqual(["opencode", "openai", "ollama"]);
    // 29 matches: "Muse Spark" has no "model" in its name, but its value has none either.
    expect(all.groups.reduce((n, g) => n + g.models.length, 0)).toBe(29);
    const many: HarnessModels = {
      ...listed,
      models: Array.from({ length: MAX_MATCHES + 25 }, (_, i) => ({ value: `p/m-${i}`, name: `m ${i}`, description: null, efforts: null })),
    };
    const capped = pickerModels(many, "m");
    expect(capped.groups.reduce((n, g) => n + g.models.length, 0)).toBe(MAX_MATCHES);
    expect(capped.more).toBe(25);
    expect(pickerModels(listed, "OPENAI 3").groups.flatMap((g) => g.models.map((m) => m.value))).toEqual(["openai/model-3"]);
  });
});
