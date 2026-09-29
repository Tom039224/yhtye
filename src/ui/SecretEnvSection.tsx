import { type FormEvent, useEffect, useRef, useState } from "react";

import { toCommandError } from "../api/transport";
import { useStore } from "../store/useStore";

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
      <p className="agent-panel-note">
        エージェントに渡す <b>秘密の環境変数</b> (API キーなど) を登録します。値は OS のキーリング (Secret Service など) に保存され、
        Yhtye は名前だけを覚えます。値は保存後に表示も取得もできません (上書きか削除だけ)。登録した変数は、これから起動する
        <b>すべてのエージェント</b> の環境に入ります。
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
        <button type="submit" className="btn" disabled={busy}>
          登録
        </button>
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
              <span className="chip secret-badge">登録済み</span>
              <span className="spacer" />
              {confirming === n ? (
                <>
                  <span className="agent-panel-status">{n} を削除しますか?</span>
                  <button type="button" className="btn btn-small btn-danger" disabled={busy} onClick={() => remove(n)}>
                    削除する
                  </button>
                  <button type="button" className="btn btn-small" onClick={() => setConfirming(null)}>
                    やめる
                  </button>
                </>
              ) : (
                <>
                  <button type="button" className="btn btn-small" aria-label={`${n} を上書き`} onClick={() => overwrite(n)}>
                    上書き
                  </button>
                  <button
                    type="button"
                    className="btn btn-small btn-danger"
                    aria-label={`${n} を削除`}
                    onClick={() => setConfirming(n)}
                  >
                    削除
                  </button>
                </>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
