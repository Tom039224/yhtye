import { useCallback, useEffect, useState } from "react";

import type { AgentRole, AgentSettingsView, Candidate, RoleSettings } from "../api/generated";
import { toCommandError } from "../api/transport";
import { useStore } from "../store/useStore";
import { CandidateTable } from "./CandidateTable";
import { Icon, IconButton, type IconName } from "./Icon";
import {
  type Edit,
  type EffortsState,
  type ModelsState,
  ROLES,
  addRow,
  choiceLabel,
  isMissing,
  knownEfforts,
  modelKey,
  newRow,
  removeRow,
  updateRow,
  withDefaultRow,
} from "./agentSettings";

export type Scope = "global" | "project";

interface Props {
  /** The open project (the "this project" scope), if any. */
  project: { id: string; name: string } | null;
  scope: Scope;
}

/**
 * The "エージェント" section of the settings (Stage 7b, tables in 7d): per role,
 * the candidate rows (harness × model × effort + a note) the orchestrator may
 * pick from and the default row, globally or for this project (a project role
 * can inherit the global one). Models and their efforts are read from the
 * harnesses by the core. Every change is saved at once and applies to agent
 * sessions started afterwards.
 */
export function AgentSettingsSection({ project, scope }: Props) {
  const store = useStore();
  const [view, setView] = useState<AgentSettingsView | null>(null);
  const [models, setModels] = useState<Record<string, ModelsState | undefined>>({});
  const [efforts, setEfforts] = useState<Record<string, EffortsState | undefined>>({});
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const scopeProject = scope === "project" ? (project?.id ?? null) : null;

  useEffect(() => {
    let live = true;
    setView(null);
    setError(null);
    store.getAgentSettings(scopeProject).then(
      (v) => live && setView(v),
      (e) => live && setError(toCommandError(e).message),
    );
    return () => {
      live = false;
    };
  }, [store, scopeProject]);

  const loadModels = useCallback(
    (harness: string, refresh: boolean) => {
      setModels((m) => ({ ...m, [harness]: { models: m[harness]?.models ?? null, error: null, loading: true } }));
      store.listHarnessModels(harness, refresh).then(
        (list) => setModels((m) => ({ ...m, [harness]: { models: list, error: null, loading: false } })),
        (e) =>
          setModels((m) => ({
            ...m,
            [harness]: { models: m[harness]?.models ?? null, error: toCommandError(e).message, loading: false },
          })),
      );
    },
    [store],
  );

  const harnessIds = view?.harnesses.map((h) => h.id).join(",") ?? "";
  useEffect(() => {
    for (const id of harnessIds.split(",").filter(Boolean)) {
      if (!models[id]) loadModels(id, false);
    }
    // Each harness is listed once (the reload button asks again), so `models`
    // is deliberately not a dependency.
  }, [harnessIds, loadModels]);

  // Efforts of models whose list entry has none (long lists) are read per model.
  const wanted = view
    ? [...new Set(ROLES.flatMap(({ role }) => view.effective[role].candidates.map((c) => needsEfforts(c, models, efforts))))]
        .filter((k): k is string => k !== null)
        .sort()
        .join("\n")
    : "";
  useEffect(() => {
    for (const key of wanted.split("\n").filter(Boolean)) {
      const [harness, ...rest] = key.split("/");
      const model = rest.join("/");
      setEfforts((e) => ({ ...e, [key]: { efforts: null, error: null, loading: true } }));
      store.listModelEfforts(harness, model).then(
        (r) => setEfforts((e) => ({ ...e, [key]: { efforts: r.efforts, error: null, loading: false } })),
        (err) => setEfforts((e) => ({ ...e, [key]: { efforts: null, error: toCommandError(err).message, loading: false } })),
      );
    }
  }, [wanted, store]);

  const save = async (role: AgentRole, settings: RoleSettings | null) => {
    setSaving(true);
    setError(null);
    try {
      setView(await store.setAgentSettings(scopeProject, role, settings));
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setSaving(false);
    }
  };
  const saveEdit = (role: AgentRole, edit: Edit) => {
    if ("error" in edit) setError(edit.error);
    else void save(role, edit.ok);
  };

  const loadingModels = Object.values(models).some((m) => m?.loading);

  return (
    <div className="agent-section">
      <div className="settings-toolbar">
        <p
          className="agent-panel-note"
          title="役割ごとに、使えるハーネス × モデル × effort の行を並べます。★ の行が既定で、オーケストレータは各行の用途メモを読んでタスクごとに行を選べます (行と完全に一致するものだけ)。変更はすぐ保存され、新しく起動するエージェントから有効です。"
        >
          <Icon name="info" size={14} />
          <span>
            <Icon name="star" size={12} className="inline-icon" /> が既定の行。用途メモを手がかりに、オーケストレータがタスクごとに行を選びます。
          </span>
        </p>
        <IconButton
          icon="refresh"
          label="モデル一覧を再取得"
          spin={loadingModels}
          disabled={!view || loadingModels}
          onClick={() => {
            setEfforts({});
            view?.harnesses.forEach((h) => loadModels(h.id, true));
          }}
        />
      </div>
      {view?.harnesses.map((h) => {
        const m = models[h.id];
        if (m?.loading) return <p key={h.id} className="agent-panel-status">{h.label}: モデル一覧を取得中…</p>;
        if (m?.error) return <p key={h.id} className="agent-panel-status error-text">{h.label}: モデル一覧を取得できません — {m.error}</p>;
        if (m?.models && m.models.models.length === 0) {
          return h.requires_model ? (
            <p key={h.id} className="agent-panel-status error-text">{h.label}: モデル一覧が空のため選べません</p>
          ) : (
            <p key={h.id} className="agent-panel-status">{h.label}: モデルを選べないハーネスです (既定のモデルを使います)</p>
          );
        }
        return null;
      })}
      {view && view.harnesses.length === 0 ? (
        <p className="agent-warning" role="alert">
          ⚠ 使えるハーネスが見つかりません。設定 › ハーネス を開いて、検出状態の確認や実行ファイルのパスの指定をしてください。
        </p>
      ) : null}
      {error ? <p className="agent-panel-status error-text" role="alert">{error}</p> : null}
      {view ? (
        <div className="agent-roles">
          {ROLES.map(({ role, label, hint, icon }) => (
            <RoleEditor
              key={role}
              role={role}
              label={label}
              hint={hint}
              icon={icon}
              view={view}
              scope={scope}
              models={models}
              efforts={efforts}
              saving={saving}
              onSave={(s) => void save(role, s)}
              onEdit={(e) => saveEdit(role, e)}
              onError={setError}
            />
          ))}
        </div>
      ) : error ? null : (
        <p className="agent-panel-status">読み込み中…</p>
      )}
    </div>
  );
}

/** `harness/model` when the efforts of the row's model are still to be read. */
function needsEfforts(
  c: Candidate,
  models: Record<string, ModelsState | undefined>,
  efforts: Record<string, EffortsState | undefined>,
): string | null {
  const listed = models[c.harness]?.models;
  if (!listed || c.model === null) return null;
  if (knownEfforts(listed, efforts, c.harness, c.model) !== null) return null;
  const key = modelKey(c.harness, c.model);
  return efforts[key] ? null : key;
}

function RoleEditor({
  role,
  label,
  hint,
  icon,
  view,
  scope,
  models,
  efforts,
  saving,
  onSave,
  onEdit,
  onError,
}: {
  role: AgentRole;
  label: string;
  hint: string;
  icon: IconName;
  view: AgentSettingsView;
  scope: Scope;
  models: Record<string, ModelsState | undefined>;
  efforts: Record<string, EffortsState | undefined>;
  saving: boolean;
  onSave: (settings: RoleSettings | null) => void;
  onEdit: (edit: Edit) => void;
  onError: (message: string) => void;
}) {
  const layer = scope === "project" ? view.project_layer : view.global;
  const own = layer?.[role] ?? null;
  const shown = view.effective[role];
  const inherits = own === null;
  const locked = saving || (scope === "project" && inherits);
  const inheritLabel = scope === "project" ? "全体の設定を使う" : "組み込みの既定を使う";
  const builtin = choiceLabel(view.builtin, view.harnesses, models[view.builtin.harness]?.models);
  const missing = [...new Set(shown.candidates.filter((c) => isMissing(c, view.harnesses)).map((c) => c.harness))];
  const add = () => {
    const row = newRow(view.harnesses, models, shown);
    if (row) onEdit(addRow(shown, row));
    else onError("追加できるハーネス × モデルがありません (すべて使われているか、一覧を取得できていません)");
  };

  return (
    <fieldset className="agent-role" aria-label={label}>
      <legend title={hint}>
        <Icon name={icon} size={15} />
        <span className="agent-role-name">{label}</span>
      </legend>
      <label className="agent-inherit" title={scope === "global" ? `組み込みの既定: ${builtin}` : undefined}>
        <input type="checkbox" checked={inherits} disabled={saving} onChange={(e) => onSave(e.target.checked ? null : shown)} />
        {inheritLabel}
      </label>
      {missing.length > 0 ? (
        <p className="agent-warning" role="alert">
          <Icon name="alert" size={14} />
          <span>
            {missing.join(" / ")} が見つかりません (インストールされていない)。該当する候補は使えず、
            代わりに既定 ({builtin}) で起動します。
          </span>
        </p>
      ) : null}
      <CandidateTable
        label={label}
        settings={shown}
        harnesses={view.harnesses}
        models={models}
        efforts={efforts}
        locked={locked}
        onChange={(i, patch) => onEdit(updateRow(shown, i, patch))}
        onDefault={(i) => onSave(withDefaultRow(shown, i))}
        onRemove={(i) => {
          const next = removeRow(shown, i);
          if (next) onSave(next);
        }}
      />
      <div>
        <IconButton icon="plus" size={15} className="icon-button-outline" label="行を追加" disabled={locked} onClick={add} />
      </div>
    </fieldset>
  );
}
