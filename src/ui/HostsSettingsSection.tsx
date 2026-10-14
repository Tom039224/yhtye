import { type FormEvent, useEffect, useState } from "react";

import type { HostToken, PairingInput, PairingPreview } from "../api/remoteHosts";
import { toCommandError } from "../api/transport";
import type { HostEntry } from "../store/hosts";
import { useHostsRegistry, useHostsState } from "../store/useHosts";
import { compatibilityNote, describePairError, type PairProblem, remoteAddress, remoteState, tokenNote } from "./hostLabels";
import { Icon, IconButton } from "./Icon";

/** The port a Yhtye listens on unless told otherwise (what an empty port box means). */
const DEFAULT_PORT = 1423;
/** What the core keeps of a device's name (`MAX_DEVICE_NAME_CHARS`). */
const MAX_DEVICE_NAME = 64;

/** A pairing link that reached the app (a click on `yhtye://pair?…`); a new request each time, even for the same link. */
export interface PairLinkRequest {
  uri: string;
  seq: number;
}

type Mode = "link" | "manual";

/** What the last pairing left to tell. */
interface Added {
  name: string;
  token: HostToken;
}

/**
 * The settings' "接続先" section (core-design.md §9, §18.9): the other Yhtye
 * hosts this PC is a client of. A form pairs with one — a `yhtye://pair?…`
 * link (also what a click on one fills in), or its host, port and code — under
 * a name for this PC that the person confirms; nothing pairs until "接続" is
 * pressed. Below it the hosts that were paired with: their state, and the
 * means to connect again or forget one.
 */
export function HostsSettingsSection({ request }: { request: PairLinkRequest | null }) {
  const registry = useHostsRegistry();
  const hosts = useHostsState((s) => s.hosts);
  const remotes = (hosts ?? []).filter((h) => h.remote);
  const [mode, setMode] = useState<Mode>("link");
  const [link, setLink] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [code, setCode] = useState("");
  // The name of this PC: until it is typed, the one the core offers (the PC's own host name).
  const [typedName, setTypedName] = useState<string | null>(null);
  const [preview, setPreview] = useState<PairingPreview | null>(null);
  const [linkProblem, setLinkProblem] = useState<PairProblem | null>(null);
  const [pairing, setPairing] = useState(false);
  const [problem, setProblem] = useState<PairProblem | null>(null);
  const [added, setAdded] = useState<Added | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [acting, setActing] = useState<string | null>(null);

  // A link that arrives (clicked, or pasted into the app by the shell) is put in the box, for the person to read.
  useEffect(() => {
    if (!request) return;
    setMode("link");
    setLink(request.uri);
    setProblem(null);
    setAdded(null);
  }, [request]);

  // What the link in the box would pair with (nothing is sent: the shell only reads it).
  const trimmedLink = link.trim();
  useEffect(() => {
    setPreview(null);
    setLinkProblem(null);
    if (!registry || mode !== "link" || trimmedLink === "") return;
    let current = true;
    registry.previewPairing({ kind: "uri", uri: trimmedLink }).then(
      (p) => current && setPreview(p),
      (e: unknown) => current && setLinkProblem(describePairError(toCommandError(e))),
    );
    return () => {
      current = false;
    };
  }, [registry, mode, trimmedLink]);

  if (!registry) return <p className="agent-panel-status">接続先の管理はデスクトップアプリでだけ使えます。</p>;

  const thisPcName = hosts?.find((h) => !h.remote)?.name ?? "";
  const deviceName = typedName ?? preview?.default_device_name ?? thisPcName;
  const input: PairingInput = mode === "link" ? { kind: "uri", uri: trimmedLink } : { kind: "manual", host: host.trim(), port: port.trim(), code: code.trim() };
  const filled = mode === "link" ? trimmedLink !== "" : host.trim() !== "" && code.trim() !== "";
  const canPair = filled && deviceName.trim() !== "" && !pairing && (mode === "manual" || linkProblem === null);

  const pair = async (e: FormEvent) => {
    e.preventDefault();
    if (!canPair) return;
    setPairing(true);
    setProblem(null);
    setAdded(null);
    setNotice(null);
    try {
      const info = await registry.addHost(input, deviceName.trim());
      setAdded({ name: info.name, token: info.token });
      setLink("");
      setCode("");
    } catch (err) {
      setProblem(describePairError(toCommandError(err)));
    } finally {
      setPairing(false);
    }
  };

  const retry = async (entry: HostEntry) => {
    setActing(entry.key);
    setError(null);
    try {
      await registry.retryHost(entry.key);
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setActing(null);
    }
  };

  const forget = async (entry: HostEntry) => {
    setActing(entry.key);
    setError(null);
    setConfirming(null);
    try {
      const outcome = await registry.removeHost(entry.key);
      setNotice(
        outcome.token_left_in_keyring
          ? `${entry.name ?? entry.key} を削除しました。ただし、キーリングのトークンを消せませんでした: ${outcome.token_left_in_keyring}`
          : `${entry.name ?? entry.key} を削除しました。ホスト側の端末一覧には、そこで失効するまで残ります。`,
      );
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setActing(null);
    }
  };

  return (
    <div className="agent-section remote-section">
      <p className="agent-panel-note">
        <Icon name="remote" size={14} />
        <span>
          別の PC やサーバーで動いている Yhtye に、この PC から接続します。つながるのは同じ Tailscale ユーザーのホストだけです。接続先で「設定 → リモート接続 → 端末を追加」
          (ヘッドレスなら <code>yhtye remote pair</code>) を開き、出てきたペアリングリンクか、ホスト・ポート・コードを入れてください。
        </span>
      </p>
      <section className="remote-block" aria-label="接続先を追加">
        <form className="host-form" onSubmit={(e) => void pair(e)} autoComplete="off">
          <div className="scope-toggle" role="group" aria-label="入力のしかた">
            <button type="button" aria-pressed={mode === "link"} disabled={pairing} onClick={() => setMode("link")}>
              ペアリングリンク
            </button>
            <button type="button" aria-pressed={mode === "manual"} disabled={pairing} onClick={() => setMode("manual")}>
              ホスト・ポート・コード
            </button>
          </div>
          {mode === "link" ? (
            <>
              <label className="host-field">
                <span>ペアリングリンク (yhtye://pair?… を貼り付け)</span>
                <input
                  className="agent-search"
                  aria-label="ペアリングリンク"
                  placeholder="yhtye://pair?v=1&…"
                  value={link}
                  spellCheck={false}
                  disabled={pairing}
                  onChange={(e) => setLink(e.target.value)}
                />
              </label>
              {preview ? <Target preview={preview} /> : null}
              {linkProblem ? <Problem problem={linkProblem} prefix="このリンクは使えません: " /> : null}
            </>
          ) : (
            <div className="host-fields">
              <label className="host-field">
                <span>ホスト (Tailscale の IP か MagicDNS 名)</span>
                <input className="agent-search" aria-label="ホスト" placeholder="100.x.y.z" value={host} spellCheck={false} disabled={pairing} onChange={(e) => setHost(e.target.value)} />
              </label>
              <label className="host-field host-field-port">
                <span>ポート</span>
                <input
                  className="agent-search"
                  aria-label="ポート"
                  inputMode="numeric"
                  placeholder={String(DEFAULT_PORT)}
                  value={port}
                  spellCheck={false}
                  disabled={pairing}
                  onChange={(e) => setPort(e.target.value)}
                />
              </label>
              <label className="host-field host-field-code">
                <span>コード</span>
                <input className="agent-search" aria-label="コード" placeholder="ABCD-2345" value={code} spellCheck={false} disabled={pairing} onChange={(e) => setCode(e.target.value)} />
              </label>
            </div>
          )}
          <label className="host-field">
            <span>この PC の名前 (接続先の端末一覧に出ます)</span>
            <input
              className="agent-search"
              aria-label="この PC の名前"
              value={deviceName}
              maxLength={MAX_DEVICE_NAME}
              spellCheck={false}
              disabled={pairing}
              onChange={(e) => setTypedName(e.target.value)}
            />
          </label>
          {problem ? <Problem problem={problem} /> : null}
          {added ? <AddedNote added={added} /> : null}
          <div className="remote-row">
            <button type="submit" className="text-button" disabled={!canPair}>
              <Icon name={pairing ? "loader" : "check"} size={13} spin={pairing} />
              接続
            </button>
            {mode === "link" && preview ? <span className="agent-panel-status">心当たりのないリンクなら「接続」を押さないでください。</span> : null}
          </div>
        </form>
      </section>
      {error ? (
        <p className="agent-warning" role="alert">
          {error}
        </p>
      ) : null}
      {notice ? (
        <p className="agent-panel-status" role="status">
          {notice}
        </p>
      ) : null}
      <section className="remote-devices" aria-label="接続先">
        <h4 className="remote-heading">接続先</h4>
        {remotes.length === 0 ? (
          <p className="agent-panel-status">接続先はありません。</p>
        ) : (
          <ul className="secret-list" aria-label="接続先の一覧">
            {remotes.map((entry) => (
              <HostRow
                key={entry.key}
                entry={entry}
                busy={acting === entry.key}
                confirming={confirming === entry.key}
                onConfirm={(on) => setConfirming(on ? entry.key : null)}
                onRetry={() => void retry(entry)}
                onForget={() => void forget(entry)}
              />
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

/** Where the link in the box leads, for the person to check before pairing. */
function Target({ preview }: { preview: PairingPreview }) {
  return (
    <dl className="remote-facts" aria-label="リンクの接続先">
      <dt>接続先</dt>
      <dd>
        {preview.hosts.map((h) => (
          <code key={h}>{h}</code>
        ))}
      </dd>
      <dt>ポート</dt>
      <dd>
        <code>{preview.port}</code>
      </dd>
    </dl>
  );
}

function Problem({ problem, prefix = "" }: { problem: PairProblem; prefix?: string }) {
  return (
    <div className="agent-warning host-problem" role="alert">
      <Icon name="alert" size={14} />
      <span>
        {prefix}
        {problem.text}
        {problem.detail ? <small className="host-detail">{problem.detail}</small> : null}
      </span>
    </div>
  );
}

/** What is said after a host was paired with (and that its token could not be kept, if so). */
function AddedNote({ added }: { added: Added }) {
  const note = tokenNote(added.token);
  return (
    <>
      <p className="agent-panel-status" role="status">
        <Icon name="check" size={12} /> {added.name} に接続しました。
      </p>
      {note ? (
        <p className="agent-warning" role="alert">
          <Icon name="alert" size={14} />
          <span>{note}</span>
        </p>
      ) : null}
    </>
  );
}

function HostRow({
  entry,
  busy,
  confirming,
  onConfirm,
  onRetry,
  onForget,
}: {
  entry: HostEntry;
  busy: boolean;
  confirming: boolean;
  onConfirm: (on: boolean) => void;
  onRetry: () => void;
  onForget: () => void;
}) {
  const info = entry.info;
  if (!info) return null;
  const name = entry.name ?? entry.key;
  const state = remoteState(entry);
  const connection = info.status.connection;
  const tone = !state.down ? "tone-done" : connection.state === "revoked" || connection.state === "rejected" ? "tone-handling" : "";
  const notes = [
    connection.state === "revoked" ? "もう一度つなぐには、この接続先を削除して、接続先で作った新しいコードで追加し直してください" : null,
    connection.state === "rejected"
      ? `接続先がこの PC を受け付けません (同じ Tailscale ユーザーの端末でない、またはタグ付きノードの可能性があります)${connection.message ? `: ${connection.message}` : ""}`
      : null,
    tokenNote(info.token),
    compatibilityNote(entry.compatibility),
  ].filter((n): n is string => n !== null);
  const canRetry = connection.state === "idle" || connection.state === "waiting_to_reconnect" || connection.state === "rejected";
  return (
    <li className="secret-row remote-device remote-host" aria-label={name}>
      <Icon name={info.status.server?.headless ? "server-headless" : "server"} size={14} className="client-kind" />
      <span className="secret-name">{name}</span>
      <span className={`chip ${tone}`}>{state.text}</span>
      <span className="harness-note">{remoteAddress(info)}</span>
      <span className="harness-note">端末名 {info.device_name}</span>
      <span className="spacer" />
      {confirming ? (
        <>
          <span className="agent-panel-status">{name} を削除しますか? この PC が忘れるだけで、ホスト側の端末一覧には残ります。</span>
          <IconButton icon="trash" size={14} tone="danger" label="削除する" disabled={busy} onClick={onForget} />
          <IconButton icon="x" size={14} label="やめる" onClick={() => onConfirm(false)} />
        </>
      ) : (
        <>
          {canRetry ? <IconButton icon="refresh" size={14} label={`${name} に再接続`} disabled={busy} onClick={onRetry} /> : null}
          <IconButton icon="trash" size={14} tone="danger" label={`${name} を削除`} disabled={busy} onClick={() => onConfirm(true)} />
        </>
      )}
      {notes.map((n) => (
        <p key={n} className="host-note">
          {n}
        </p>
      ))}
    </li>
  );
}
