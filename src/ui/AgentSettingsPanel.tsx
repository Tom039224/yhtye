import { useCallback, useEffect, useState } from "react";

import type { AgentChoice, AgentRole, AgentSettingsView, RoleSettings } from "../api/generated";
import { toCommandError } from "../api/transport";
import { useStore } from "../store/useStore";
import {
  type ModelsState,
  ROLES,
  choiceKey,
  choiceLabel,
  choiceOptions,
  isMissing,
  sameChoice,
  toggleCandidate,
  visibleOptions,
  withDefault,
  writesAsOrchestrator,
} from "./agentSettings";

/** Marks an orchestrator option whose harness cannot be made read-only. */
const NO_WRITE_LIMIT = "⚠ 書き込み制限なし";

type Scope = "global" | "project";

interface Props {
  /** The open project (the "this project" scope), if any. */
  project: { id: string; name: string } | null;
  onClose: () => void;
}

/**
 * The ⚙ panel of the composer (Stage 7b): per role, the harness × model
 * candidates the orchestrator may pick from and the default, globally or for
 * this project (a project role can inherit the global one). Models are read
 * from the harnesses by the core. Every change is saved at once and applies
 * to agent sessions started afterwards.
 */
export function AgentSettingsPanel({ project, onClose }: Props) {
  const store = useStore();
  const [chosenScope, setScope] = useState<Scope>(project ? "project" : "global");
  const scope: Scope = project ? chosenScope : "global";
  const [view, setView] = useState<AgentSettingsView | null>(null);
  const [models, setModels] = useState<Record<string, ModelsState | undefined>>({});
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState<AgentRole | null>(null);
  const scopeProject = scope === "project" ? (project?.id ?? null) : null;

  useEffect(() => {
    let live = true;
    setView(null);
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

  const save = async (role: AgentRole, settings: RoleSettings | null) => {
    setSaving(role);
    setError(null);
    try {
      setView(await store.setAgentSettings(scopeProject, role, settings));
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setSaving(null);
    }
  };

  return (
    <div className="agent-panel" role="dialog" aria-label="エージェントの設定">
      <div className="agent-panel-head">
        <span className="agent-panel-title">エージェント (ハーネス × モデル)</span>
        <div className="scope-toggle" role="group" aria-label="設定の範囲">
          <button type="button" aria-pressed={scope === "global"} onClick={() => setScope("global")}>
            全体
          </button>
          <button
            type="button"
            aria-pressed={scope === "project"}
            disabled={!project}
            title={project ? project.name : "プロジェクトを開いていません"}
            onClick={() => setScope("project")}
          >
            このプロジェクト{project ? ` (${project.name})` : ""}
          </button>
        </div>
        <span className="spacer" />
        <button
          type="button"
          className="btn btn-small"
          disabled={!view || Object.values(models).some((m) => m?.loading)}
          onClick={() => view?.harnesses.forEach((h) => loadModels(h.id, true))}
        >
          モデル一覧を再取得
        </button>
        <button type="button" className="icon-btn" aria-label="閉じる" onClick={onClose}>
          ×
        </button>
      </div>
      <p className="agent-panel-note">
        変更はすぐ保存され、新しく起動するエージェントから有効です (動いているエージェントはそのまま)。
        オーケストレータはタスクごとに候補の中から選べます。
      </p>
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
      {error ? <p className="agent-panel-status error-text" role="alert">{error}</p> : null}
      {view ? (
        <div className="agent-roles">
          {ROLES.map(({ role, label, hint }) => (
            <RoleEditor
              key={role}
              role={role}
              label={label}
              hint={hint}
              view={view}
              scope={scope}
              models={models}
              saving={saving !== null}
              onSave={(s) => void save(role, s)}
            />
          ))}
        </div>
      ) : error ? null : (
        <p className="agent-panel-status">読み込み中…</p>
      )}
    </div>
  );
}

function RoleEditor({
  role,
  label,
  hint,
  view,
  scope,
  models,
  saving,
  onSave,
}: {
  role: AgentRole;
  label: string;
  hint: string;
  view: AgentSettingsView;
  scope: Scope;
  models: Record<string, ModelsState | undefined>;
  saving: boolean;
  onSave: (settings: RoleSettings | null) => void;
}) {
  const [query, setQuery] = useState("");
  const layer = scope === "project" ? view.project_layer : view.global;
  const own = layer?.[role] ?? null;
  const shown = view.effective[role];
  const inherits = own === null;
  const locked = saving || (scope === "project" && inherits);
  const options = choiceOptions(view.harnesses, models, shown.candidates);
  const visible = visibleOptions(options, view.harnesses, models, shown.candidates, query);
  const toggle = (choice: AgentChoice, on: boolean) => {
    const next = toggleCandidate(shown, choice, on);
    if (next) onSave(next);
  };
  const inheritLabel = scope === "project" ? "全体の設定を使う" : "組み込みの既定を使う";
  const builtin = choiceLabel(view.builtin, view.harnesses, models[view.builtin.harness]?.models);
  const isOrchestrator = role === "orchestrator";
  const unlimited = (c: AgentChoice) => isOrchestrator && writesAsOrchestrator(c, view.harnesses);
  const optionText = (c: AgentChoice) => {
    const text = choiceLabel(c, view.harnesses, models[c.harness]?.models);
    return unlimited(c) ? `${text} ${NO_WRITE_LIMIT}` : text;
  };
  const writers = [...new Set(shown.candidates.filter(unlimited).map((c) => view.harnesses.find((h) => h.id === c.harness)?.label ?? c.harness))];
  const missing = [...new Set(shown.candidates.filter((c) => isMissing(c, view.harnesses)).map((c) => c.harness))];

  return (
    <fieldset className="agent-role" aria-label={label}>
      <legend>
        <span className="agent-role-name">{label}</span>
        <span className="agent-role-hint">{hint}</span>
      </legend>
      <label className="agent-inherit" title={scope === "global" ? `組み込みの既定: ${builtin}` : undefined}>
        <input
          type="checkbox"
          checked={inherits}
          disabled={saving}
          onChange={(e) => onSave(e.target.checked ? null : shown)}
        />
        {inheritLabel}
      </label>
      {writers.length > 0 ? (
        <p className="agent-warning" role="note">
          ⚠ {writers.join(" / ")} のオーケストレータは書き込みを制限できません。ファイル編集やコマンド実行ができ、
          「自分で書かない」はプロンプトで指示しているだけです。
        </p>
      ) : null}
      {missing.length > 0 ? (
        <p className="agent-warning" role="alert">
          ⚠ {missing.join(" / ")} が見つかりません (インストールされていない)。該当する候補は使えず、
          代わりに既定 ({builtin}) で起動します。
        </p>
      ) : null}
      <input
        type="search"
        className="agent-search"
        aria-label={`${label} のモデルを検索`}
        placeholder="モデルを検索 (例: free, gpt)"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      <ul className="agent-candidates" aria-label={`${label} の候補`}>
        {visible.groups.map((g) => (
          <li key={g.key} className="agent-group">
            {visible.groups.length > 1 ? <span className="agent-group-label">{g.label}</span> : null}
            <ul className="agent-candidates">
              {g.options.map((o) => {
                const checked = shown.candidates.some((c) => sameChoice(c, o.choice));
                const last = checked && shown.candidates.length === 1;
                const gone = isMissing(o.choice, view.harnesses);
                return (
                  <li key={choiceKey(o.choice)}>
                    <label title={last ? "候補は 1 つ以上必要です" : undefined}>
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={locked || last}
                        onChange={(e) => toggle(o.choice, e.target.checked)}
                      />
                      {o.label}
                      {unlimited(o.choice) ? <span className="agent-warn-tag"> {NO_WRITE_LIMIT}</span> : null}
                      {gone ? <span className="agent-warn-tag"> (見つからない)</span> : null}
                      {o.unlisted ? <span className="agent-unlisted"> (一覧に無い)</span> : null}
                    </label>
                  </li>
                );
              })}
            </ul>
          </li>
        ))}
      </ul>
      {visible.more > 0 ? <p className="agent-panel-status">他に {visible.more} 件 — 検索語を絞ってください</p> : null}
      {visible.searchable.map((h) => (
        <p key={h.label} className="agent-panel-status">
          {h.label}: {h.count} モデル — 検索して候補に追加
        </p>
      ))}
      <label className="agent-default">
        既定
        <select
          aria-label={`${label} の既定`}
          value={choiceKey(shown.default)}
          disabled={locked}
          onChange={(e) => {
            const choice = shown.candidates.find((c) => choiceKey(c) === e.target.value);
            if (choice) onSave(withDefault(shown, choice));
          }}
        >
          {shown.candidates.map((c) => (
            <option key={choiceKey(c)} value={choiceKey(c)}>
              {optionText(c)}
            </option>
          ))}
        </select>
      </label>
    </fieldset>
  );
}
