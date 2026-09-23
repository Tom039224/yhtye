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
    // A harness that requires a model (OpenCode) has no usable "default" entry.
    const fallback: AgentChoice[] = h.requires_model ? [] : [{ harness: h.id, model: null }];
    const choices: AgentChoice[] =
      listed.models.length > 0 ? listed.models.map((m) => ({ harness: h.id, model: m.value })) : fallback;
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

/** Harnesses with more models than this are searched instead of listed in full. */
export const LONG_LIST = 12;
/** Most search matches shown per role (the rest: refine the search). */
export const MAX_MATCHES = 40;

/** A heading and its options in a role's list. */
export interface OptionGroup {
  key: string;
  /** Shown when there is more than one group. */
  label: string;
  options: ChoiceOption[];
}

export interface VisibleOptions {
  groups: OptionGroup[];
  /** Matches not shown (over {@link MAX_MATCHES}). */
  more: number;
  /** Long-listed harnesses whose models are only shown when searched: label and count. */
  searchable: { label: string; count: number }[];
}

function isLong(models: Record<string, ModelsState | undefined>, harness: string): boolean {
  return (models[harness]?.models?.models.length ?? 0) > LONG_LIST;
}

/** Whether every whitespace-separated word of `query` is in the option's label or value. */
export function matches(option: ChoiceOption, query: string): boolean {
  const hay = `${option.label} ${option.choice.model ?? ""}`.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((w) => hay.includes(w));
}

/**
 * What a role's list shows: its candidates always; other options of short
 * lists when they match `query` (all when it is empty); other options of long
 * lists (OpenCode's hundreds of models) only when searched, at most
 * {@link MAX_MATCHES}. Options are grouped by harness and, for long lists, by
 * provider (the `<provider>/` prefix of the model).
 */
export function visibleOptions(
  options: ChoiceOption[],
  harnesses: HarnessInfo[],
  models: Record<string, ModelsState | undefined>,
  candidates: AgentChoice[],
  query: string,
): VisibleOptions {
  const q = query.trim();
  const shown: ChoiceOption[] = [];
  let matched = 0;
  for (const o of options) {
    const checked = candidates.some((c) => sameChoice(c, o.choice));
    if (checked) {
      shown.push(o);
      continue;
    }
    if (q === "" ? isLong(models, o.choice.harness) : !matches(o, q)) continue;
    matched += 1;
    if (matched <= MAX_MATCHES) shown.push(o);
  }
  const groups: OptionGroup[] = [];
  for (const o of shown) {
    const harness = harnesses.find((h) => h.id === o.choice.harness)?.label ?? o.choice.harness;
    const model = o.choice.model ?? "";
    const provider = isLong(models, o.choice.harness) && model.includes("/") ? model.slice(0, model.indexOf("/")) : null;
    const key = `${o.choice.harness}/${provider ?? ""}`;
    const group = groups.find((g) => g.key === key);
    if (group) group.options.push(o);
    else groups.push({ key, label: provider ? `${harness} · ${provider}` : harness, options: [o] });
  }
  const searchable =
    q === ""
      ? harnesses
          .filter((h) => isLong(models, h.id))
          .map((h) => ({ label: h.label, count: models[h.id]?.models?.models.length ?? 0 }))
      : [];
  return { groups, more: Math.max(0, matched - MAX_MATCHES), searchable };
}

/** The harness of `choice` is not registered (e.g. OpenCode is no longer installed). */
export function isMissing(choice: AgentChoice, harnesses: HarnessInfo[]): boolean {
  return !harnesses.some((h) => h.id === choice.harness);
}

/** As orchestrator, `choice`'s harness can write files (only the prompt forbids it). */
export function writesAsOrchestrator(choice: AgentChoice, harnesses: HarnessInfo[]): boolean {
  return harnesses.some((h) => h.id === choice.harness && !h.orchestrator_read_only);
}
