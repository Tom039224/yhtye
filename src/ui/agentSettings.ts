// Pure helpers of the agent settings (Stage 7b, extended in 7d; core-design.md
// §15): the roles, the candidate rows of a role (harness × model × effort +
// a note, one of them the default) and the edits of those rows.

import type { AgentChoice, AgentRole, Candidate, EffortOption, HarnessInfo, HarnessModels, ModelOption, RoleSettings } from "../api/generated";

export const ROLES: { role: AgentRole; label: string; hint: string }[] = [
  { role: "orchestrator", label: "オーケストレータ", hint: "あなたと話し、タスクを組む" },
  { role: "implementer", label: "実装", hint: "code タスク" },
  { role: "investigator", label: "調査", hint: "investigate タスク" },
  { role: "reviewer", label: "レビュー", hint: "review Step" },
];

/** Longest note (characters) the core accepts. */
export const MAX_NOTE_CHARS = 400;

/** What is known about one harness's models in the settings. */
export interface ModelsState {
  models: HarnessModels | null;
  error: string | null;
  loading: boolean;
}

/** What is known about the efforts of one model whose list entry has none. */
export interface EffortsState {
  efforts: EffortOption[] | null;
  error: string | null;
  loading: boolean;
}

/** The key of a `(harness, model)` in the per-model effort cache. */
export function modelKey(harness: string, model: string): string {
  return `${harness}/${model}`;
}

type Triple = Pick<AgentChoice, "harness" | "model" | "effort">;

/** The harness × model × effort of a row (without its note). */
export function rowChoice(c: Candidate): AgentChoice {
  return { harness: c.harness, model: c.model, effort: c.effort };
}

export function sameChoice(a: Triple, b: Triple): boolean {
  return a.harness === b.harness && (a.model ?? null) === (b.model ?? null) && (a.effort ?? null) === (b.effort ?? null);
}

/** `Name (value)`, or just the value when the harness gives no separate name. */
export function modelText(model: string, listed?: HarnessModels | null): string {
  const name = listed?.models.find((m) => m.value === model)?.name;
  return name && name !== model ? `${name} (${model})` : model;
}

/** `Claude Code · Sonnet (sonnet) · effort high` (for messages). */
export function choiceLabel(c: AgentChoice, harnesses: HarnessInfo[], models?: HarnessModels | null): string {
  const harness = harnesses.find((h) => h.id === c.harness)?.label ?? c.harness;
  const model = c.model === null ? "既定のモデル" : modelText(c.model, models);
  return `${harness} · ${model}${c.effort ? ` · effort ${c.effort}` : ""}`;
}

/** The outcome of an edit: the new settings, or why the edit is not possible. */
export type Edit = { ok: RoleSettings } | { error: string };

export const DUPLICATE_ROW = "同じハーネス・モデル・effort の行が既にあります (行ごとに組み合わせを変えてください)";

function hasTriple(rows: Candidate[], choice: Triple, except = -1): boolean {
  return rows.some((r, i) => i !== except && sameChoice(r, choice));
}

/**
 * `settings` with row `index` changed by `patch`. The default follows the row
 * if it was the default. A change that makes two rows the same harness × model
 * × effort is refused.
 */
export function updateRow(settings: RoleSettings, index: number, patch: Partial<Candidate>): Edit {
  const old = settings.candidates[index];
  if (!old) return { error: "行が見つかりません" };
  const row: Candidate = { ...old, ...patch };
  if (hasTriple(settings.candidates, row, index)) return { error: DUPLICATE_ROW };
  const candidates = settings.candidates.map((r, i) => (i === index ? row : r));
  const wasDefault = sameChoice(old, settings.default);
  return { ok: { candidates, default: wasDefault ? rowChoice(row) : settings.default } };
}

/** `settings` with `row` appended (refused when the same combination exists). */
export function addRow(settings: RoleSettings, row: Candidate): Edit {
  if (hasTriple(settings.candidates, row)) return { error: DUPLICATE_ROW };
  return { ok: { ...settings, candidates: [...settings.candidates, row] } };
}

/**
 * `settings` without row `index`. Removing the default makes the first
 * remaining row the default; removing the last row is not possible (`null`).
 */
export function removeRow(settings: RoleSettings, index: number): RoleSettings | null {
  const candidates = settings.candidates.filter((_, i) => i !== index);
  if (candidates.length === 0 || candidates.length === settings.candidates.length) return null;
  const keepDefault = candidates.some((c) => sameChoice(c, settings.default));
  return { candidates, default: keepDefault ? settings.default : rowChoice(candidates[0]) };
}

/** `settings` with row `index` as the default. */
export function withDefaultRow(settings: RoleSettings, index: number): RoleSettings {
  const row = settings.candidates[index];
  return row ? { ...settings, default: rowChoice(row) } : settings;
}

/** Harnesses with more models than this get a searchable picker instead of a plain select. */
export const LONG_LIST = 12;
/** Most search matches shown (the rest: refine the search). */
export const MAX_MATCHES = 40;

export function isLongList(listed: HarnessModels | null | undefined): boolean {
  return (listed?.models.length ?? 0) > LONG_LIST;
}

/** Whether every whitespace-separated word of `query` is in the model's name or value. */
export function matchesModel(model: ModelOption, query: string): boolean {
  const hay = `${model.name} ${model.value}`.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((w) => hay.includes(w));
}

/** A provider heading (the `<provider>/` prefix of the model) and its models. */
export interface ModelGroup {
  provider: string | null;
  models: ModelOption[];
}

export interface PickerResult {
  groups: ModelGroup[];
  /** Matches not shown (over {@link MAX_MATCHES}). */
  more: number;
}

/**
 * The models a long-list picker shows for `query`: those matching all its
 * words (none for an empty query: hundreds of models are only searched), at
 * most {@link MAX_MATCHES}, grouped by provider.
 */
export function pickerModels(listed: HarnessModels, query: string): PickerResult {
  if (query.trim() === "") return { groups: [], more: 0 };
  const matched = listed.models.filter((m) => matchesModel(m, query));
  const groups: ModelGroup[] = [];
  for (const m of matched.slice(0, MAX_MATCHES)) {
    const provider = m.value.includes("/") ? m.value.slice(0, m.value.indexOf("/")) : null;
    const group = groups.find((g) => g.provider === provider);
    if (group) group.models.push(m);
    else groups.push({ provider, models: [m] });
  }
  return { groups, more: Math.max(0, matched.length - MAX_MATCHES) };
}

/** The efforts known for `model` of a harness, or `null` while they are not known yet. */
export function knownEfforts(
  listed: HarnessModels | null | undefined,
  cache: Record<string, EffortsState | undefined>,
  harness: string,
  model: string | null,
): EffortOption[] | null {
  if (model === null) return [];
  const fromList = listed?.models.find((m) => m.value === model)?.efforts;
  if (fromList) return fromList;
  return cache[modelKey(harness, model)]?.efforts ?? null;
}

/**
 * A new row: the first harness × model (none as effort) that no row has yet,
 * starting with the harness of the last row. `null` when none is left or the
 * lists are not loaded.
 */
export function newRow(
  harnesses: HarnessInfo[],
  models: Record<string, ModelsState | undefined>,
  settings: RoleSettings,
): Candidate | null {
  const last = settings.candidates.at(-1)?.harness;
  const ordered = [...harnesses].sort((a, b) => Number(b.id === last) - Number(a.id === last));
  for (const h of ordered) {
    const listed = models[h.id]?.models;
    if (!listed) continue;
    const values: (string | null)[] = listed.models.length > 0 ? listed.models.map((m) => m.value) : h.requires_model ? [] : [null];
    for (const model of values) {
      const row: Candidate = { harness: h.id, model, effort: null, note: "" };
      if (!hasTriple(settings.candidates, row)) return row;
    }
  }
  return null;
}

/** The first model a row gets when its harness changes (`undefined`: none can be chosen). */
export function firstModel(harness: HarnessInfo, listed: HarnessModels | null | undefined): string | null | undefined {
  if (!listed) return undefined;
  if (listed.models.length > 0) return listed.models[0].value;
  return harness.requires_model ? undefined : null;
}

/** The harness of `choice` is not registered (e.g. OpenCode is no longer installed). */
export function isMissing(choice: Pick<AgentChoice, "harness">, harnesses: HarnessInfo[]): boolean {
  return !harnesses.some((h) => h.id === choice.harness);
}

/** As orchestrator, `choice`'s harness can write files (only the prompt forbids it). */
export function writesAsOrchestrator(choice: Pick<AgentChoice, "harness">, harnesses: HarnessInfo[]): boolean {
  return harnesses.some((h) => h.id === choice.harness && !h.orchestrator_read_only);
}
