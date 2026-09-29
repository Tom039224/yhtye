import { useEffect, useId, useRef, useState } from "react";

import type { HarnessModels } from "../api/generated";
import { modelText, pickerModels } from "./agentSettings";

interface Props {
  /** Names the trigger button (`… のモデル`). */
  label: string;
  value: string | null;
  listed: HarnessModels;
  disabled: boolean;
  onChange: (model: string) => void;
}

/**
 * The model picker of a harness with a long model list (OpenCode: hundreds):
 * a button that opens a search box with the matches grouped by provider
 * (Stage 7c-2, kept in 7d). Short lists use a plain select instead.
 */
export function ModelPicker({ label, value, listed, disabled, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const root = useRef<HTMLDivElement>(null);
  const listId = useId();
  const result = pickerModels(listed, query);

  useEffect(() => {
    if (!open) return;
    const outside = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", outside);
    return () => document.removeEventListener("mousedown", outside);
  }, [open]);

  const close = () => {
    setOpen(false);
    setQuery("");
  };
  const choose = (model: string) => {
    close();
    onChange(model);
  };

  return (
    <div className="model-picker" ref={root}>
      <button
        type="button"
        className="picker-button"
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled}
        title={value ?? undefined}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="picker-value">{value === null ? "既定のモデル" : modelText(value, listed)}</span>
        <span aria-hidden="true">▾</span>
      </button>
      {open ? (
        <div
          className="picker-popover"
          onKeyDown={(e) => {
            // Escape closes only the picker, not the whole settings dialog.
            if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              close();
            }
          }}
        >
          <input
            type="search"
            className="agent-search"
            // eslint-disable-next-line jsx-a11y/no-autofocus
            autoFocus
            aria-label={`${label}を検索`}
            aria-controls={listId}
            placeholder={`${listed.models.length} モデルから検索 (例: free, gpt)`}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <div id={listId} className="picker-list" role="listbox" aria-label={`${label}の候補`}>
            {query.trim() === "" ? <p className="agent-panel-status">検索語を入力してください (空白区切りで AND)</p> : null}
            {query.trim() !== "" && result.groups.length === 0 ? <p className="agent-panel-status">一致するモデルがありません</p> : null}
            {result.groups.map((g) => (
              <div key={g.provider ?? ""} role="group" aria-label={g.provider ?? "その他"}>
                {result.groups.length > 1 ? <span className="agent-group-label">{g.provider ?? "その他"}</span> : null}
                {g.models.map((m) => (
                  <button
                    type="button"
                    role="option"
                    key={m.value}
                    aria-selected={m.value === value}
                    className={`picker-option ${m.value === value ? "selected" : ""}`}
                    title={m.description ?? m.value}
                    onClick={() => choose(m.value)}
                  >
                    {modelText(m.value, listed)}
                  </button>
                ))}
              </div>
            ))}
            {result.more > 0 ? <p className="agent-panel-status">他に {result.more} 件 — 検索語を絞ってください</p> : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}
