import { type FormEvent, useEffect, useState } from "react";

import type { HarnessDetection, HarnessPathSource } from "../api/generated";
import { toCommandError } from "../api/transport";
import { useStore } from "../store/useStore";
import { Icon, IconButton } from "./Icon";

/** Where the main executable was found, as shown. */
export const PATH_SOURCE_LABELS: Record<HarnessPathSource, string> = {
  override: "手動",
  path: "PATH",
  known_dir: "既知の場所",
  none: "—",
};

/** The manual-path text boxes' contents: each harness's stored override, if any. */
function draftsOf(list: HarnessDetection[]): Record<string, string> {
  return Object.fromEntries(list.map((h) => [h.id, h.override_path ?? ""]));
}

/**
 * The settings' "ハーネス" section: whether each harness (Claude Code, OpenCode,
 * Codex, Devin, MiniMax Code, Google Antigravity) is installed, where its commands
 * were found, and a manual path for its main executable that replaces the
 * automatic search (PATH, then ~/.local/bin, ~/.cargo/bin, ~/.bun/bin,
 * /usr/local/bin, and the harness's own places: MiniMax Code's
 * ~/.minimax-code/bin, Antigravity's ~/.local/share/agy-acp-server). Opening it
 * makes the core look again; the agent settings offer the installed harnesses.
 */
export function HarnessSettingsSection() {
  const store = useStore();
  const [harnesses, setHarnesses] = useState<HarnessDetection[] | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [rowErrors, setRowErrors] = useState<Record<string, string | undefined>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [detecting, setDetecting] = useState(false);

  useEffect(() => {
    let live = true;
    store.getHarnesses().then(
      (list) => {
        if (!live) return;
        setHarnesses(list);
        setDrafts(draftsOf(list));
      },
      (e) => live && setError(toCommandError(e).message),
    );
    return () => {
      live = false;
    };
  }, [store]);

  const detect = async () => {
    setBusy(true);
    setDetecting(true);
    setError(null);
    setRowErrors({});
    try {
      const list = await store.detectHarnesses();
      // The first look failed: nothing filled the boxes yet. Otherwise keep what was typed.
      if (harnesses === null) setDrafts(draftsOf(list));
      setHarnesses(list);
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
      setDetecting(false);
    }
  };

  /** Saves (or with `null` removes) the manual path of `harness`. */
  const setPath = async (harness: string, path: string | null) => {
    setBusy(true);
    setRowErrors((r) => ({ ...r, [harness]: undefined }));
    try {
      const list = await store.setHarnessPath(harness, path);
      setHarnesses(list);
      setDrafts((d) => ({ ...d, [harness]: list.find((h) => h.id === harness)?.override_path ?? "" }));
    } catch (e) {
      setRowErrors((r) => ({ ...r, [harness]: toCommandError(e).message }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="agent-section harness-section">
      <div className="settings-toolbar">
        <p
          className="agent-panel-note"
          title="エージェントを動かすハーネスの検出状態です。実行ファイルは PATH と既知の場所 (~/.local/bin, ~/.cargo/bin, ~/.bun/bin, /usr/local/bin) から探します。見つからないときは主実行ファイルの絶対パスを手動で指定できます (手動パスは自動検出より優先)。インストール済みのハーネスがエージェント設定で選べます。"
        >
          <Icon name="info" size={14} />
          <span>PATH と既知の場所から探します。見つからなければ絶対パスを手動で指定できます (自動検出より優先)。</span>
        </p>
        <IconButton
          icon="refresh"
          label="再検出"
          title="ハーネスを再検出"
          spin={detecting || (harnesses === null && error === null)}
          disabled={busy || (harnesses === null && error === null)}
          onClick={detect}
        />
      </div>
      {error ? (
        <p className="agent-warning" role="alert">
          {error}
        </p>
      ) : null}
      {harnesses === null ? (
        error ? null : <p className="agent-panel-status">検出中…</p>
      ) : (
        <ul className="secret-list" aria-label="ハーネス">
          {harnesses.map((h) => (
            <HarnessRow
              key={h.id}
              harness={h}
              draft={drafts[h.id] ?? ""}
              error={rowErrors[h.id]}
              busy={busy}
              onDraft={(value) => setDrafts((d) => ({ ...d, [h.id]: value }))}
              onError={(message) => setRowErrors((r) => ({ ...r, [h.id]: message }))}
              onSetPath={(path) => void setPath(h.id, path)}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function HarnessRow({
  harness: h,
  draft,
  error,
  busy,
  onDraft,
  onError,
  onSetPath,
}: {
  harness: HarnessDetection;
  draft: string;
  error: string | undefined;
  busy: boolean;
  onDraft: (value: string) => void;
  onError: (message: string) => void;
  onSetPath: (path: string | null) => void;
}) {
  const main = h.requirements[0]?.command ?? h.id;
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const path = draft.trim();
    if (path) onSetPath(path);
    else onError("パスを入力してください (自動検出にするには「自動検出に戻す」)");
  };

  return (
    <li className="secret-row harness-row" aria-label={h.label}>
      <div className="harness-head">
        <span className="secret-name">{h.label}</span>
        <code className="harness-id">{h.id}</code>
        <span className={`chip ${h.installed ? "tone-done" : "tone-handling"}`}>
          <Icon name={h.installed ? "check" : "alert"} size={12} />
          {h.installed ? "インストール済み" : "未インストール"}
        </span>
      </div>
      <ul className="harness-reqs" aria-label={`${h.label} の必要なコマンド`}>
        {h.requirements.map((r, i) => (
          <li key={r.command} className="harness-req">
            <code className="harness-command">{r.command}</code>
            {r.found ? <code className="harness-path">{r.found}</code> : <span className="error-text">見つかりません</span>}
            {i === 0 ? (
              r.found ? <span className="chip">{PATH_SOURCE_LABELS[h.path_source]}</span> : null
            ) : (
              <span className="harness-note">自動検出のみ</span>
            )}
          </li>
        ))}
      </ul>
      {h.override_error ? (
        <p className="agent-warning" role="alert">
          <Icon name="alert" size={14} />
          <span>
            手動パス {h.override_path} を使えません: {h.override_error}
          </span>
        </p>
      ) : null}
      {error ? (
        <p className="agent-warning" role="alert">
          {error}
        </p>
      ) : null}
      <form className="secret-form" aria-label={`${h.label} の手動パス`} onSubmit={submit} autoComplete="off">
        <input
          className="agent-search"
          aria-label={`${h.label} の ${main} のパス`}
          placeholder={`${main} の絶対パス (例 /opt/bin/${main})`}
          value={draft}
          spellCheck={false}
          onChange={(e) => onDraft(e.target.value)}
        />
        <IconButton type="submit" icon="check" size={15} className="icon-button-outline" label="保存" disabled={busy} />
        <IconButton
          icon="x"
          size={15}
          className="icon-button-outline"
          label="自動検出に戻す"
          title="手動パスを消して自動検出に戻す"
          disabled={busy || h.override_path === null}
          onClick={() => onSetPath(null)}
        />
      </form>
    </li>
  );
}
