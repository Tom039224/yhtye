// Pure helpers of the agent settings panel (Stage 7b, core-design.md §15):
// the roles, the harness × model options offered for them, and edits of a
// role's candidates / default.

import type { AgentChoice, AgentRole, HarnessInfo, HarnessModels, RoleSettings } from "../api/generated";

export const ROLES: { role: AgentRole; label: string; hint: string }[] = [
  { role: "orchestrator", label: "オーケストレータ", hint: "あなたと話し、タスクを組む" },
  { role: "implementer", label: "実装", hint: "code タスク" },
  { role: "investigator", label: "調査", hint: "investigate タスク" },
  { role: "reviewer", label: "レビュー", hint: "review Step" },
];

/** What is known about one harness's models in the panel. */
export interface ModelsState {
  models: HarnessModels | null;
  error: string | null;
  loading: boolean;
}

/** One checkbox of a role's candidate list. */
export interface ChoiceOption {
  choice: AgentChoice;
  label: string;
  /** Not in the harness's current model list (kept so it can be unchecked). */
  unlisted: boolean;
}

export function sameChoice(a: AgentChoice, b: AgentChoice): boolean {
  return a.harness === b.harness && (a.model ?? null) === (b.model ?? null);
}

export function choiceKey(c: AgentChoice): string {
  return `${c.harness}/${c.model ?? ""}`;
}

export function choiceLabel(c: AgentChoice, harnesses: HarnessInfo[], models?: HarnessModels | null): string {
  const harness = harnesses.find((h) => h.id === c.harness)?.label ?? c.harness;
  if (c.model === null) return `${harness} · 既定のモデル`;
  const name = models?.models.find((m) => m.value === c.model)?.name;
  return `${harness} · ${name && name !== c.model ? `${name} (${c.model})` : c.model}`;
}

/**
 * The options of a role: every listed model of every harness (a harness
 * without a model option offers only its default), then candidates that are
 * not listed (unknown yet or no longer offered).
 */
export function choiceOptions(
  harnesses: HarnessInfo[],
  models: Record<string, ModelsState | undefined>,
  candidates: AgentChoice[],
): ChoiceOption[] {
  const out: ChoiceOption[] = [];
  for (const h of harnesses) {
    const listed = models[h.id]?.models;
    if (!listed) continue;
    const choices: AgentChoice[] =
      listed.models.length > 0 ? listed.models.map((m) => ({ harness: h.id, model: m.value })) : [{ harness: h.id, model: null }];
    for (const choice of choices) out.push({ choice, label: choiceLabel(choice, harnesses, listed), unlisted: false });
  }
  for (const c of candidates) {
    if (!out.some((o) => sameChoice(o.choice, c))) {
      const listed = models[c.harness]?.models ?? null;
      out.push({ choice: c, label: choiceLabel(c, harnesses, listed), unlisted: listed !== null });
    }
  }
  return out;
}

/**
 * `settings` with `choice` added to or removed from the candidates. Removing
 * the default makes the first remaining candidate the default; removing the
 * last candidate is not possible (`null`).
 */
export function toggleCandidate(settings: RoleSettings, choice: AgentChoice, on: boolean): RoleSettings | null {
  const has = settings.candidates.some((c) => sameChoice(c, choice));
  if (on) {
    return has ? settings : { ...settings, candidates: [...settings.candidates, choice] };
  }
  const candidates = settings.candidates.filter((c) => !sameChoice(c, choice));
  if (candidates.length === 0) return null;
  const keepDefault = candidates.some((c) => sameChoice(c, settings.default));
  return { candidates, default: keepDefault ? settings.default : candidates[0] };
}

/** `settings` with `choice` (one of the candidates) as the default. */
export function withDefault(settings: RoleSettings, choice: AgentChoice): RoleSettings {
  return { ...settings, default: choice };
}
