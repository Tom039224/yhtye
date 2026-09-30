import { type FormEvent, useEffect, useRef, useState } from "react";

import { toCommandError } from "../api/transport";
import { useStore } from "../store/useStore";
import { Icon, IconButton } from "./Icon";

/** Longest accepted name (the core checks the same). */
export const MAX_SECRET_NAME = 128;
const NAME_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

/** A message when `name` cannot be an environment variable name, else `null`. */
export function secretNameError(name: string): string | null {
  if (!name) return "名前を入力してください";
  if (name.length > MAX_SECRET_NAME) return `名前は ${MAX_SECRET_NAME} 文字までです`;
  if (!NAME_RE.test(name)) return "名前は英数字と _ だけで、数字から始められません";
  return null;
}

/**
 * The settings' "秘密の環境変数" section (Stage 7e): names of variables whose
 * values live in the OS keyring and are passed to every agent's environment.
 * Values are only sent (once) by the form and are never shown or kept: the
 * core returns names only. Failures (e.g. no keyring) are shown inline.
 */
export function SecretEnvSection() {
  const store = useStore();
  const [names, setNames] = useState<string[] | null>(null);
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const valueInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let live = true;
    store.listSecretEnv().then(
      (n) => live && setNames(n),
      (e) => live && setError(toCommandError(e).message),
    );
    return () => {
      live = false;
    };
  }, [store]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const trimmed = name.trim();
    const problem = secretNameError(trimmed) ?? (value === "" ? "値を入力してください" : null);
    if (problem) {
      setError(problem);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      setNames(await store.setSecretEnv(trimmed, value));
      setName("");
      setValue("");
    } catch (err) {
      // The value is dropped on failure too: nothing secret stays in the page.
      setValue("");
      setError(toCommandError(err).message);
    } finally {
      setBusy(false);
    }
  };

  const remove = async (target: string) => {
    setBusy(true);
    setError(null);
    setConfirming(null);
    try {
      setNames(await store.deleteSecretEnv(target));
    } catch (err) {
      setError(toCommandError(err).message);
    } finally {
      setBusy(false);
    }
  };

  const overwrite = (target: string) => {
    setName(target);
    setValue("");
    setError(null);
    valueInput.current?.focus();
  };

  return (
    <div className="agent-section secret-env-section">
      <p
        className="agent-panel-note"
        title="エージェントに渡す秘密の環境変数 (API キーなど) を登録します。値は OS のキーリング (Secret Service など) に保存され、Yhtye は名前だけを覚えます。値は保存後に表示も取得もできません (上書きか削除だけ)。登録した変数は、これから起動するすべてのエージェントの環境に入ります。"
      >
        <Icon name="key" size={14} />
        <span>値は OS のキーリングに保存され、保存後は見られません。登録した変数は、すべてのエージェントの環境に入ります。</span>
      </p>
      {error ? (
        <p className="agent-warning" role="alert">
          {error}
        </p>
      ) : null}
      <form className="secret-form" aria-label="秘密の環境変数を登録" onSubmit={submit} autoComplete="off">
        <input
          className="agent-search"
          aria-label="変数名"
          placeholder="NAME (例 OPENROUTER_API_KEY_CODEX)"
          value={name}
          spellCheck={false}
          onChange={(e) => setName(e.target.value)}
        />
        <input
          ref={valueInput}
          className="agent-search"
          type="password"
          aria-label="値"
          placeholder="値"
          value={value}
          autoComplete="new-password"
          onChange={(e) => setValue(e.target.value)}
        />
        <IconButton type="submit" icon="plus" size={15} className="icon-button-outline" label="登録" disabled={busy} />
      </form>
      {names === null ? (
        error ? null : <p className="agent-panel-status">読み込み中…</p>
      ) : names.length === 0 ? (
        <p className="agent-panel-status">登録されている変数はありません。</p>
      ) : (
        <ul className="secret-list" aria-label="登録済みの環境変数">
          {names.map((n) => (
            <li key={n} className="secret-row">
              <code className="secret-name">{n}</code>
              <span className="chip secret-badge" title="登録済み">
                <Icon name="check" size={12} />
                <span className="sr-only">登録済み</span>
              </span>
              <span className="spacer" />
              {confirming === n ? (
                <>
                  <span className="agent-panel-status">{n} を削除しますか?</span>
                  <IconButton icon="trash" size={14} tone="danger" label="削除する" disabled={busy} onClick={() => remove(n)} />
                  <IconButton icon="x" size={14} label="やめる" onClick={() => setConfirming(null)} />
                </>
              ) : (
                <>
                  <IconButton icon="edit" size={14} label={`${n} を上書き`} onClick={() => overwrite(n)} />
                  <IconButton icon="trash" size={14} tone="danger" label={`${n} を削除`} onClick={() => setConfirming(n)} />
                </>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
