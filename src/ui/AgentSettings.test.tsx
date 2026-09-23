// The composer's ⚙ agent settings panel (Stage 7b): roles with candidates
// (harness × model from the harness's model list) and a default, the global
// and project scopes, inheritance, and the edits it saves.

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../App";
import type { AgentChoice, RoleSettings } from "../api/generated";
import { AppStore } from "../store/app";
import { StoreContext } from "../store/useStore";
import { FULL_RUN, PROJECT } from "../test/fixtures";
import { FakeCore, MemoryTransport } from "../test/memoryTransport";
import { choiceOptions, toggleCandidate } from "./agentSettings";

const HAIKU: AgentChoice = { harness: "claude-code", model: "haiku" };
const SONNET: AgentChoice = { harness: "claude-code", model: "sonnet" };

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
      core.agents.harnesses.push({ id: "opencode", label: "OpenCode" });
      core.agents.global.implementer = {
        candidates: [HAIKU, { harness: "opencode", model: null }],
        default: HAIKU,
      };
    });
    const { panel } = await openPanel();
    await waitFor(() => expect(panel).toHaveTextContent("OpenCode: モデル一覧を取得できません"));
    expect(within(role(panel, "実装")).getByRole("checkbox", { name: "OpenCode · 既定のモデル" })).toBeChecked();
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
      { id: "a", label: "A" },
      { id: "b", label: "B" },
    ];
    const models = {
      a: { models: { harness: "a", models: [{ value: "x", name: "x", description: null }], current: "x", fetched_at_ms: 0 }, error: null, loading: false },
      b: { models: { harness: "b", models: [], current: null, fetched_at_ms: 0 }, error: null, loading: false },
    };
    const opts = choiceOptions(harnesses, models, [{ harness: "a", model: "gone" }]);
    expect(opts.map((o) => [o.label, o.unlisted])).toEqual([
      ["A · x", false],
      ["B · 既定のモデル", false],
      ["A · gone", true],
    ]);
  });
});
