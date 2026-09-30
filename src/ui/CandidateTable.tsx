import type { Candidate, EffortOption, HarnessInfo, HarnessModels, RoleSettings } from "../api/generated";
import {
  type EffortsState,
  MAX_NOTE_CHARS,
  type ModelsState,
  firstModel,
  isLongList,
  isMissing,
  knownEfforts,
  modelKey,
  modelText,
  sameChoice,
} from "./agentSettings";
import { Icon, IconButton } from "./Icon";
import { ModelPicker } from "./ModelPicker";

interface TableProps {
  /** The role's name (labels of the controls). */
  label: string;
  settings: RoleSettings;
  harnesses: HarnessInfo[];
  models: Record<string, ModelsState | undefined>;
  efforts: Record<string, EffortsState | undefined>;
  /** Nothing can be edited (saving, or inherited settings). */
  locked: boolean;
  onChange: (index: number, patch: Partial<Candidate>) => void;
  onDefault: (index: number) => void;
  onRemove: (index: number) => void;
}

/**
 * The candidate rows of one role as a table (Stage 7d): default (★) radio,
 * harness, model, effort, and a free-text note saying when to use the row.
 */
export function CandidateTable(p: TableProps) {
  const { settings } = p;
  return (
    <table className="candidate-table" aria-label={`${p.label} の候補`}>
      <thead>
        <tr>
          <th scope="col" className="col-default" title="既定">
            <Icon name="star" size={13} />
            <span className="sr-only">既定</span>
          </th>
          <th scope="col">ハーネス</th>
          <th scope="col">モデル</th>
          <th scope="col">effort</th>
          <th scope="col" title="オーケストレータが行を選ぶ手がかり">
            用途メモ
          </th>
          <th scope="col" className="col-remove">
            <span className="sr-only">削除</span>
          </th>
        </tr>
      </thead>
      <tbody>
        {settings.candidates.map((row, i) => (
          <CandidateRow key={i} index={i} row={row} isDefault={sameChoice(row, settings.default)} only={settings.candidates.length === 1} {...p} />
        ))}
      </tbody>
    </table>
  );
}

function CandidateRow({
  index,
  row,
  isDefault,
  only,
  label,
  harnesses,
  models,
  efforts,
  locked,
  onChange,
  onDefault,
  onRemove,
}: TableProps & { index: number; row: Candidate; isDefault: boolean; only: boolean }) {
  const n = index + 1;
  const missing = isMissing(row, harnesses);
  const listed = models[row.harness]?.models ?? null;

  const changeHarness = (id: string) => {
    const h = harnesses.find((x) => x.id === id);
    const model = h ? firstModel(h, models[id]?.models) : undefined;
    if (model !== undefined) onChange(index, { harness: id, model, effort: null });
  };
  const changeModel = (model: string) => {
    const keep = knownEfforts(listed, efforts, row.harness, model)?.some((e) => e.value === row.effort) ?? false;
    onChange(index, { model, effort: keep ? row.effort : null });
  };

  return (
    <tr className={isDefault ? "default-row" : undefined}>
      <td className="col-default">
        <input
          type="radio"
          name={`${label}-default`}
          aria-label={`${label} 候補${n} を既定にする`}
          checked={isDefault}
          disabled={locked}
          onChange={() => onDefault(index)}
        />
      </td>
      <td>
        <select
          aria-label={`${label} 候補${n} のハーネス`}
          value={row.harness}
          disabled={locked}
          onChange={(e) => changeHarness(e.target.value)}
        >
          {missing ? <option value={row.harness}>{row.harness} (見つからない)</option> : null}
          {harnesses.map((h) => (
            <option key={h.id} value={h.id} disabled={h.id !== row.harness && firstModel(h, models[h.id]?.models) === undefined}>
              {h.label}
            </option>
          ))}
        </select>
        {missing ? (
          <span className="agent-warn-tag" title="見つかりません (インストールされていない)">
            <Icon name="alert" size={13} />
            <span className="sr-only">見つかりません</span>
          </span>
        ) : null}
      </td>
      <td>
        <ModelCell label={`${label} 候補${n} のモデル`} row={row} listed={listed} disabled={locked || missing} onChange={changeModel} />
      </td>
      <td>
        <EffortCell
          label={`${label} 候補${n} の effort`}
          row={row}
          listed={listed}
          efforts={efforts}
          disabled={locked || missing}
          onChange={(effort) => onChange(index, { effort })}
        />
      </td>
      <td className="col-note">
        <input
          type="text"
          key={row.note}
          className="note-input"
          aria-label={`${label} 候補${n} の用途メモ`}
          placeholder="例: 設計が絡む難しい変更に"
          maxLength={MAX_NOTE_CHARS}
          defaultValue={row.note}
          disabled={locked}
          onBlur={(e) => e.target.value !== row.note && onChange(index, { note: e.target.value })}
          onKeyDown={(e) => e.key === "Enter" && !e.nativeEvent.isComposing && e.currentTarget.blur()}
        />
      </td>
      <td className="col-remove">
        <IconButton
          icon="trash"
          size={14}
          label={`${label} 候補${n} を削除`}
          title={only ? "候補は 1 行以上必要です" : "この行を削除"}
          disabled={locked || only}
          onClick={() => onRemove(index)}
        />
      </td>
    </tr>
  );
}

function ModelCell({
  label,
  row,
  listed,
  disabled,
  onChange,
}: {
  label: string;
  row: Candidate;
  listed: HarnessModels | null;
  disabled: boolean;
  onChange: (model: string) => void;
}) {
  if (!listed || listed.models.length === 0) {
    return <span className="cell-text">{row.model === null ? "既定のモデル" : row.model}</span>;
  }
  if (isLongList(listed)) {
    return <ModelPicker label={label} value={row.model} listed={listed} disabled={disabled} onChange={onChange} />;
  }
  const unlisted = row.model !== null && !listed.models.some((m) => m.value === row.model);
  return (
    <select aria-label={label} value={row.model ?? ""} disabled={disabled} onChange={(e) => onChange(e.target.value)}>
      {row.model === null ? <option value="">既定のモデル</option> : null}
      {unlisted && row.model !== null ? <option value={row.model}>{row.model} (一覧に無い)</option> : null}
      {listed.models.map((m) => (
        <option key={m.value} value={m.value}>
          {modelText(m.value, listed)}
        </option>
      ))}
    </select>
  );
}

function effortText(e: EffortOption): string {
  return e.name && e.name !== e.value ? `${e.name} (${e.value})` : e.value;
}

function EffortCell({
  label,
  row,
  listed,
  efforts,
  disabled,
  onChange,
}: {
  label: string;
  row: Candidate;
  listed: HarnessModels | null;
  efforts: Record<string, EffortsState | undefined>;
  disabled: boolean;
  onChange: (effort: string | null) => void;
}) {
  const known = knownEfforts(listed, efforts, row.harness, row.model);
  const state = row.model === null ? undefined : efforts[modelKey(row.harness, row.model)];
  const none = <option value="">指定なし</option>;
  const current = row.effort;
  if (known === null || known.length === 0) {
    // Not known yet / failed / the model has no effort: only the current value can stay.
    const hint = known === null ? (state?.error ? `effort を取得できません — ${state.error}` : "effort を読み込み中…") : "このモデルに effort はありません";
    return (
      <select aria-label={label} title={hint} value={current ?? ""} disabled onChange={() => undefined}>
        {none}
        {current ? <option value={current}>{current} (一覧に無い)</option> : null}
      </select>
    );
  }
  const unlisted = current !== null && !known.some((e) => e.value === current);
  return (
    <select aria-label={label} value={current ?? ""} disabled={disabled} onChange={(e) => onChange(e.target.value === "" ? null : e.target.value)}>
      {none}
      {unlisted && current ? <option value={current}>{current} (一覧に無い)</option> : null}
      {known.map((e) => (
        <option key={e.value} value={e.value}>
          {effortText(e)}
        </option>
      ))}
    </select>
  );
}
