// The composer's ⚙ agent settings panel (Stage 7b): roles with candidates
// (harness × model from the harness's model list) and a default, the global
// and project scopes, inheritance, and the edits it saves.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../App";
import type { AgentChoice, HarnessInfo, HarnessModels, RoleSettings } from "../api/generated";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { MAX_MATCHES, choiceOptions, toggleCandidate, visibleOptions } from "./agentSettings";

const HAIKU: AgentChoice = { harness: "claude-code", model: "haiku" };
const SONNET: AgentChoice = { harness: "claude-code", model: "sonnet" };
const FREE = "opencode/muse-spark-1.3-contributor-free";
const OPENCODE: HarnessInfo = { id: "opencode", label: "OpenCode", requires_model: true, orchestrator_read_only: false };
const PLAIN = { requires_model: false, orchestrator_read_only: true };

/** OpenCode's long model list in miniature: 3 providers × 10 models (> LONG_LIST). */
function opencodeModels(): HarnessModels {
  const models = ["opencode", "openai", "ollama"].flatMap((p) =>
    Array.from({ length: 10 }, (_, i) => ({ value: `${p}/model-${i}`, name: `${p} model ${i}`, description: null })),
  );
  models[0] = { value: FREE, name: "Muse Spark 1.3 (free)", description: null };
  return { harness: "opencode", models, current: "openai/model-1", fetched_at_ms: 0 };
}

function withOpenCode(core: FakeCore) {
  core.agents.harnesses.push(OPENCODE);
  core.agents.models.opencode = opencodeModels();
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

async function openPanel() {
  const user = userEvent.setup();
  const gear = await screen.findByRole("button", { name: "エージェントの設定" });
  await waitFor(() => expect(gear).toBeEnabled());
  await user.click(gear);
  const panel = await screen.findByRole("dialog", { name: "エージェントの設定" });
  return { user, panel };
}

function role(panel: HTMLElement, name: string): HTMLElement {
  return within(panel).getByRole("group", { name });
}

describe("agent settings panel", () => {
  it("shows every role with the harness's models, for this project by default", async () => {
    const { transport } = await setup();
    const { panel } = await openPanel();
    await waitFor(() => expect(within(role(panel, "実装")).getAllByRole("checkbox")).toHaveLength(4));
    for (const name of ["オーケストレータ", "実装", "調査", "レビュー"]) {
      const group = role(panel, name);
      expect(within(group).getByRole("checkbox", { name: "全体の設定を使う" })).toBeChecked();
      expect(within(group).getByRole("checkbox", { name: "Claude Code · Haiku (haiku)" })).toBeChecked();
      expect(within(group).getByRole("checkbox", { name: "Claude Code · Sonnet (sonnet)" })).not.toBeChecked();
    }
    expect(transport.callsOf("get_agent_settings")[0].project).toBe(PROJECT.id);
    expect(transport.callsOf("list_harness_models")).toEqual([{ type: "list_harness_models", harness: "claude-code", refresh: false }]);
    expect(panel).toHaveTextContent("新しく起動するエージェントから有効");
  });

  it("overrides a role for the project, adds a candidate and changes the default", async () => {
    const { transport, core } = await setup();
    const { user, panel } = await openPanel();
    const implementer = () => role(panel, "実装");
    await waitFor(() => expect(within(implementer()).getAllByRole("checkbox")).toHaveLength(4));
    // Inherited settings cannot be edited until the role stops inheriting.
    expect(within(implementer()).getByRole("checkbox", { name: /Sonnet/ })).toBeDisabled();

    await user.click(within(implementer()).getByRole("checkbox", { name: "全体の設定を使う" }));
    await waitFor(() => expect(within(implementer()).getByRole("checkbox", { name: /Sonnet/ })).toBeEnabled());
    await user.click(within(implementer()).getByRole("checkbox", { name: /Sonnet/ }));
    await waitFor(() => expect(within(implementer()).getByRole("checkbox", { name: /Sonnet/ })).toBeChecked());
    await user.selectOptions(within(implementer()).getByRole("combobox", { name: "実装 の既定" }), "claude-code/sonnet");

    const sets = transport.callsOf("set_agent_settings");
    expect(sets.map((s) => [s.project, s.role, s.settings])).toEqual([
      [PROJECT.id, "implementer", { candidates: [HAIKU], default: HAIKU }],
      [PROJECT.id, "implementer", { candidates: [HAIKU, SONNET], default: HAIKU }],
      [PROJECT.id, "implementer", { candidates: [HAIKU, SONNET], default: SONNET }],
    ]);
    expect(core.agents.projects.get(PROJECT.id)?.implementer?.default).toEqual(SONNET);
    expect(core.agents.global.implementer).toBeNull();
    // The only remaining candidate cannot be unchecked.
    expect(within(role(panel, "レビュー")).getByRole("checkbox", { name: /Haiku/ })).toBeDisabled();
  });

  it("edits the global layer in the global scope", async () => {
    const { transport, core } = await setup();
    const { user, panel } = await openPanel();
    await user.click(within(panel).getByRole("button", { name: "全体" }));
    await waitFor(() => expect(transport.callsOf("get_agent_settings").at(-1)?.project).toBeUndefined());
    const reviewer = () => role(panel, "レビュー");
    await waitFor(() => expect(within(reviewer()).getByRole("checkbox", { name: "組み込みの既定を使う" })).toBeChecked());
    await user.click(within(reviewer()).getByRole("checkbox", { name: /Sonnet/ }));
    await waitFor(() => expect(core.agents.global.reviewer).toEqual({ candidates: [HAIKU, SONNET], default: HAIKU }));
    expect(transport.callsOf("set_agent_settings")[0].project).toBeUndefined();
    await user.click(within(reviewer()).getByRole("checkbox", { name: "組み込みの既定を使う" }));
    await waitFor(() => expect(core.agents.global.reviewer).toBeNull());
  });

  it("reports a harness whose models cannot be read and keeps saved candidates", async () => {
    await setup((core) => {
      core.agents.harnesses.push({ id: "other", label: "Other", ...PLAIN });
      core.agents.global.implementer = {
        candidates: [HAIKU, { harness: "other", model: null }],
        default: HAIKU,
      };
    });
    const { panel } = await openPanel();
    await waitFor(() => expect(panel).toHaveTextContent("Other: モデル一覧を取得できません"));
    expect(within(role(panel, "実装")).getByRole("checkbox", { name: "Other · 既定のモデル" })).toBeChecked();
  });

  it("searches OpenCode's long model list, grouped by provider, and adds a match", async () => {
    const { transport, core } = await setup(withOpenCode);
    const { user, panel } = await openPanel();
    await user.click(within(panel).getByRole("button", { name: "全体" }));
    const implementer = () => role(panel, "実装");
    await waitFor(() => expect(implementer()).toHaveTextContent("OpenCode: 30 モデル — 検索して候補に追加"));
    // Only Claude Code's short list is shown until OpenCode is searched.
    expect(within(implementer()).getAllByRole("checkbox").map((c) => c.getAttribute("aria-label") ?? "")).toHaveLength(4);
    expect(within(implementer()).queryByRole("checkbox", { name: /OpenCode/ })).toBeNull();
    expect(transport.callsOf("list_harness_models").map((c) => c.harness)).toEqual(["claude-code", "opencode"]);

    await user.type(within(implementer()).getByRole("searchbox", { name: "実装 のモデルを検索" }), "model 1");
    const found = within(implementer()).getAllByRole("checkbox", { name: /OpenCode/ });
    expect(found).toHaveLength(3);
    expect(implementer()).toHaveTextContent("OpenCode · openai");
    expect(implementer()).toHaveTextContent("OpenCode · ollama");
    expect(within(implementer()).queryByRole("checkbox", { name: /Sonnet/ })).toBeNull();

    await user.click(within(implementer()).getByRole("checkbox", { name: "組み込みの既定を使う" }));
    await user.clear(within(implementer()).getByRole("searchbox", { name: "実装 のモデルを検索" }));
    await user.type(within(implementer()).getByRole("searchbox", { name: "実装 のモデルを検索" }), "free");
    await user.click(within(implementer()).getByRole("checkbox", { name: `OpenCode · Muse Spark 1.3 (free) (${FREE})` }));
    const free: AgentChoice = { harness: "opencode", model: FREE };
    await waitFor(() => expect(core.agents.global.implementer).toEqual({ candidates: [HAIKU, free], default: HAIKU }));
    // A chosen candidate stays listed after the search is cleared.
    await user.clear(within(implementer()).getByRole("searchbox", { name: "実装 のモデルを検索" }));
    expect(within(implementer()).getByRole("checkbox", { name: /Muse Spark/ })).toBeChecked();
    // No "default model" entry for OpenCode (it would start on its last-used model).
    expect(within(implementer()).queryByRole("checkbox", { name: "OpenCode · 既定のモデル" })).toBeNull();
  });

  it("warns that an OpenCode orchestrator cannot be made read-only", async () => {
    const free: AgentChoice = { harness: "opencode", model: FREE };
    await setup((core) => {
      withOpenCode(core);
      core.agents.global.orchestrator = { candidates: [HAIKU, free], default: HAIKU };
      core.agents.global.implementer = { candidates: [HAIKU, free], default: HAIKU };
    });
    const { panel } = await openPanel();
    const orchestrator = role(panel, "オーケストレータ");
    await waitFor(() =>
      expect(within(orchestrator).getByRole("checkbox", { name: /Muse Spark.*⚠ 書き込み制限なし/ })).toBeChecked(),
    );
    expect(within(orchestrator).getByRole("note")).toHaveTextContent("OpenCode のオーケストレータは書き込みを制限できません");
    expect(within(orchestrator).getByRole("option", { name: /Muse Spark.*⚠ 書き込み制限なし/ })).toBeInTheDocument();
    // Offered (searched) OpenCode models are marked too, Claude Code is not.
    const user = userEvent.setup();
    await user.type(within(orchestrator).getByRole("searchbox"), "openai model 2");
    expect(within(orchestrator).getByRole("checkbox", { name: /openai model 2.*⚠ 書き込み制限なし/ })).not.toBeChecked();
    expect(within(orchestrator).getByRole("checkbox", { name: /Haiku/ }).parentElement).not.toHaveTextContent("書き込み制限なし");
    // Other roles: no warning.
    const implementer = role(panel, "実装");
    expect(within(implementer).queryByRole("note")).toBeNull();
    expect(implementer).not.toHaveTextContent("書き込み制限なし");
  });

  it("shows settings that name a harness which is no longer installed", async () => {
    await setup((core) => {
      core.agents.global.implementer = {
        candidates: [HAIKU, { harness: "opencode", model: FREE }],
        default: { harness: "opencode", model: FREE },
      };
    });
    const { panel } = await openPanel();
    const implementer = role(panel, "実装");
    await waitFor(() => expect(within(implementer).getByRole("alert")).toHaveTextContent("opencode が見つかりません"));
    expect(within(implementer).getByRole("alert")).toHaveTextContent("既定 (Claude Code · Haiku (haiku)) で起動します");
    expect(within(implementer).getByRole("checkbox", { name: /opencode.*\(見つからない\)/ })).toBeChecked();
    expect(within(role(panel, "レビュー")).queryByRole("alert")).toBeNull();
  });

  it("closes with ×", async () => {
    await setup();
    const { user, panel } = await openPanel();
    await user.click(within(panel).getByRole("button", { name: "閉じる" }));
    expect(screen.queryByRole("dialog", { name: "エージェントの設定" })).toBeNull();
  });
});

describe("agent settings helpers", () => {
  const only = (c: AgentChoice): RoleSettings => ({ candidates: [c], default: c });

  it("keep at least one candidate and move the default off a removed one", () => {
    expect(toggleCandidate(only(HAIKU), HAIKU, false)).toBeNull();
    const both = toggleCandidate(only(HAIKU), SONNET, true);
    expect(both).toEqual({ candidates: [HAIKU, SONNET], default: HAIKU });
    expect(toggleCandidate(both!, HAIKU, false)).toEqual({ candidates: [SONNET], default: SONNET });
    expect(toggleCandidate(both!, SONNET, true)).toBe(both);
  });

  it("offer a harness without models as its default and mark unlisted candidates", () => {
    const harnesses = [
      { id: "a", label: "A", ...PLAIN },
      { id: "b", label: "B", ...PLAIN },
      { id: "c", label: "C", ...PLAIN, requires_model: true },
    ];
    const models = {
      a: { models: { harness: "a", models: [{ value: "x", name: "x", description: null }], current: "x", fetched_at_ms: 0 }, error: null, loading: false },
      b: { models: { harness: "b", models: [], current: null, fetched_at_ms: 0 }, error: null, loading: false },
      c: { models: { harness: "c", models: [], current: null, fetched_at_ms: 0 }, error: null, loading: false },
    };
    const opts = choiceOptions(harnesses, models, [{ harness: "a", model: "gone" }]);
    expect(opts.map((o) => [o.label, o.unlisted])).toEqual([
      ["A · x", false],
      ["B · 既定のモデル", false],
      ["A · gone", true],
    ]);
  });

  it("limit search matches and always keep the candidates", () => {
    const harnesses = [OPENCODE];
    const listed = opencodeModels();
    const models = { opencode: { models: listed, error: null, loading: false } };
    const chosen: AgentChoice = { harness: "opencode", model: "ollama/model-9" };
    const opts = choiceOptions(harnesses, models, [chosen]);
    expect(opts).toHaveLength(30);
    const idle = visibleOptions(opts, harnesses, models, [chosen], "");
    expect(idle.groups.flatMap((g) => g.options.map((o) => o.choice.model))).toEqual(["ollama/model-9"]);
    expect(idle.searchable).toEqual([{ label: "OpenCode", count: 30 }]);
    const all = visibleOptions(opts, harnesses, models, [chosen], "model");
    expect(all.groups.map((g) => g.label)).toEqual(["OpenCode · opencode", "OpenCode · openai", "OpenCode · ollama"]);
    // 28 matches ("Muse Spark" has no "model" in it) + the candidate.
    expect(all.groups.reduce((n, g) => n + g.options.length, 0)).toBe(29);
    expect(all.more).toBe(0);
    const many: HarnessModels = {
      ...listed,
      models: Array.from({ length: MAX_MATCHES + 25 }, (_, i) => ({ value: `p/m-${i}`, name: `m ${i}`, description: null })),
    };
    const big = { opencode: { models: many, error: null, loading: false } };
    const capped = visibleOptions(choiceOptions(harnesses, big, []), harnesses, big, [], "m");
    expect(capped.groups.reduce((n, g) => n + g.options.length, 0)).toBe(MAX_MATCHES);
    expect(capped.more).toBe(25);
    const words = visibleOptions(opts, harnesses, models, [], "OPENAI 3");
    expect(words.groups.flatMap((g) => g.options.map((o) => o.choice.model))).toEqual(["openai/model-3"]);
  });
});
