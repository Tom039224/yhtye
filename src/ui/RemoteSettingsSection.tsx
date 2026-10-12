import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";

import type { PairingCode, RemoteDevice, RemoteStatus } from "../api/generated";
import { toCommandError } from "../api/transport";
import { useStore } from "../store/useStore";
import { Icon, IconButton } from "./Icon";
import { formatDateTime } from "./labels";
import { QrCode } from "./QrCode";
import { useNow } from "./useNow";

const MAX_PORT = 65535;
const PORT_MESSAGE = `ポートは 1〜${MAX_PORT} の数字で入力してください`;

/** A message when `text` is not a port (1-65535), else `null`. */
export function portError(text: string): string | null {
  const t = text.trim();
  if (!/^\d{1,5}$/.test(t)) return PORT_MESSAGE;
  const n = Number(t);
  return n < 1 || n > MAX_PORT ? PORT_MESSAGE : null;
}

/** An 8-character pairing code in two halves (`ABCD-2345`), easier to read out and type. */
export function formatPairingCode(code: string): string {
  return code.length === 8 ? `${code.slice(0, 4)}-${code.slice(4)}` : code;
}

/** Time left as `m:ss` (never below 0:00). */
export function formatRemaining(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** The pairing code on screen. */
interface Pairing {
  code: PairingCode;
  /** The core has reported it as the valid code (its event may come before or after the reply). */
  confirmed: boolean;
  /** The core no longer reports it although time is left: a device paired with it, or it was guessed wrong too often. */
  used: boolean;
}

/**
 * What becomes of the code on screen when the core reports `status`. Running
 * out of time sends no event; the card compares `expires_at_ms` with the clock.
 */
function nextPairing(p: Pairing | null, status: RemoteStatus, now: number): Pairing | null {
  if (!p || p.used) return p;
  if (!status.running) return null;
  if (status.pairing_expires_at_ms === p.code.expires_at_ms) return p.confirmed ? p : { ...p, confirmed: true };
  return p.confirmed && now < p.code.expires_at_ms ? { ...p, used: true } : p;
}

/**
 * The settings' "リモート接続" section (core-design.md §18): the server for the
 * Android client over Tailscale — on/off and its port, where it listens, a
 * pairing code as a QR code (and for typing in), and the paired devices with
 * their revocation. The core's `remote_status_changed` / `remote_devices_changed`
 * events keep it current. A server that fails to start shows the core's reason.
 */
export function RemoteSettingsSection() {
  const store = useStore();
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [devices, setDevices] = useState<RemoteDevice[] | null>(null);
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [portDraft, setPortDraft] = useState("");
  const [portProblem, setPortProblem] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [switching, setSwitching] = useState(false);
  const [confirming, setConfirming] = useState<string | null>(null);
  const statusRef = useRef<RemoteStatus | null>(null);

  const applyStatus = useCallback((next: RemoteStatus) => {
    statusRef.current = next;
    setStatus(next);
    setPairing((p) => nextPairing(p, next, Date.now()));
  }, []);

  useEffect(
    () =>
      store.onRemoteEvent((body) => {
        if (body.type === "remote_status_changed") applyStatus(body.status);
        else setDevices(body.devices);
      }),
    [store, applyStatus],
  );

  useEffect(() => {
    let live = true;
    Promise.all([store.getRemoteStatus(), store.listRemoteDevices()]).then(
      ([s, d]) => {
        if (!live) return;
        applyStatus(s);
        setDevices(d);
      },
      (e) => live && setError(toCommandError(e).message),
    );
    return () => {
      live = false;
    };
  }, [store, applyStatus]);

  // The box follows the stored port (also when it changes elsewhere).
  const storedPort = status?.port;
  useEffect(() => {
    if (storedPort !== undefined) setPortDraft(String(storedPort));
  }, [storedPort]);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  };

  const apply = (enabled: boolean, port?: number) =>
    run(async () => {
      setSwitching(true);
      try {
        applyStatus(await store.setRemoteSettings(enabled, port));
      } finally {
        setSwitching(false);
      }
    });

  /** The port typed in, or `null` (and the reason shown) when it is not one. */
  const typedPort = (): number | null => {
    const problem = portError(portDraft);
    setPortProblem(problem);
    return problem ? null : Number(portDraft.trim());
  };

  const toggle = (on: boolean) => {
    if (!on) return void apply(false);
    const port = typedPort();
    if (port !== null) void apply(true, port);
  };

  const savePort = (e: FormEvent) => {
    e.preventDefault();
    const port = typedPort();
    if (port !== null && status) void apply(status.enabled, port);
  };

  const createCode = () =>
    run(async () => {
      const code = await store.createPairingCode();
      setPairing({ code, confirmed: statusRef.current?.pairing_expires_at_ms === code.expires_at_ms, used: false });
    });

  const revoke = (device: string) =>
    run(async () => {
      setConfirming(null);
      setDevices(await store.revokeRemoteDevice(device));
    });

  return (
    <div className="agent-section remote-section">
      <p
        className="agent-panel-note"
        title="Android アプリからこの PC の Yhtye を操作できるようにします。待ち受けるのはこの PC の Tailscale のアドレスだけで、つながるのはペアリングした端末 (同じ Tailscale ユーザーのもの) だけです。リモート接続の設定・ペアリング・失効はこの画面からだけできます。"
      >
        <Icon name="phone" size={14} />
        <span>Android 端末から Tailscale 経由で操作できるようにします。待ち受けは Tailscale のアドレスだけ、つながるのはペアリングした端末だけです。</span>
      </p>
      {error ? (
        <p className="agent-warning" role="alert">
          {error}
        </p>
      ) : null}
      {status === null ? (
        error ? null : <p className="agent-panel-status">読み込み中…</p>
      ) : (
        <section className="remote-block" aria-label="サーバー">
          <div className="remote-row">
            <label className="remote-switch">
              <input type="checkbox" role="switch" checked={status.enabled} disabled={busy} onChange={(e) => toggle(e.target.checked)} />
              <span>リモート接続</span>
            </label>
            <ServerState status={status} switching={switching} />
            <span className="spacer" />
            <form className="remote-port" aria-label="ポート" onSubmit={savePort} autoComplete="off">
              <input
                className="agent-search"
                aria-label="ポート"
                inputMode="numeric"
                aria-invalid={portProblem ? true : undefined}
                value={portDraft}
                spellCheck={false}
                onChange={(e) => {
                  setPortDraft(e.target.value);
                  setPortProblem(null);
                }}
              />
              <IconButton type="submit" icon="check" size={15} className="icon-button-outline" label="ポートを保存" disabled={busy} />
            </form>
          </div>
          {portProblem ? (
            <p className="agent-warning" role="alert">
              {portProblem}
            </p>
          ) : null}
          {status.running ? (
            <dl className="remote-facts">
              <dt>待ち受け</dt>
              <dd>
                {status.addresses.map((a) => (
                  <code key={a}>{a}</code>
                ))}
              </dd>
              <dt>MagicDNS</dt>
              <dd>{status.dns_name ? <code>{status.dns_name}</code> : "無効 (IP で接続)"}</dd>
            </dl>
          ) : status.enabled && status.error ? (
            <div className="agent-warning remote-error" role="alert">
              <Icon name="alert" size={14} />
              <span>起動できません: {status.error}</span>
              <button type="button" className="text-button" disabled={busy} onClick={() => void apply(true)}>
                <Icon name="refresh" size={13} />
                再試行
              </button>
            </div>
          ) : null}
          {status.running ? (
            pairing ? (
              <PairingCard
                pairing={pairing}
                host={status.dns_name ?? status.ip ?? "—"}
                port={status.port}
                busy={busy}
                onNew={createCode}
                onHide={() => setPairing(null)}
              />
            ) : (
              <div className="remote-row">
                <button type="button" className="text-button" disabled={busy} onClick={createCode}>
                  <Icon name="plus" size={13} />
                  端末を追加
                </button>
              </div>
            )
          ) : null}
        </section>
      )}
      <DeviceList devices={devices} busy={busy} confirming={confirming} onConfirm={setConfirming} onRevoke={(id) => void revoke(id)} />
    </div>
  );
}

/** The server's state; `switching`: a change of the settings is being applied (starting may take seconds). */
function ServerState({ status, switching }: { status: RemoteStatus; switching: boolean }) {
  const [tone, icon, text] = status.running
    ? (["tone-done", "check", "待ち受け中"] as const)
    : !status.enabled
      ? (["", "stop", "停止中"] as const)
      : status.error
        ? (["tone-handling", "alert", "エラー"] as const)
        : (["", "loader", "開始中…"] as const);
  return (
    <span className={`chip ${tone}`} role="status">
      <Icon name={switching ? "loader" : icon} size={12} spin={switching} />
      {text}
    </span>
  );
}

function PairingCard({
  pairing,
  host,
  port,
  busy,
  onNew,
  onHide,
}: {
  pairing: Pairing;
  host: string;
  port: number;
  busy: boolean;
  onNew: () => void;
  onHide: () => void;
}) {
  const now = useNow(1000);
  const left = pairing.code.expires_at_ms - now;
  const ended = pairing.used ? "このコードは使われたか、無効になりました。" : left <= 0 ? "コードの有効期限が切れました。" : null;
  return (
    <section className="remote-pairing" aria-label="ペアリングコード">
      {ended === null ? (
        <>
          <QrCode value={pairing.code.uri} label="ペアリング用の QR コード" />
          <div className="remote-manual">
            <p className="agent-panel-status">Android アプリで QR コードを読むか、次を入力してください。</p>
            <dl className="remote-facts">
              <dt>ホスト</dt>
              <dd>
                <code>{host}</code>
              </dd>
              <dt>ポート</dt>
              <dd>
                <code>{port}</code>
              </dd>
              <dt>コード</dt>
              <dd>
                <code className="remote-code">{formatPairingCode(pairing.code.code)}</code>
              </dd>
            </dl>
            <p className="agent-panel-status">
              <Icon name="hourglass" size={12} /> 残り {formatRemaining(left)} (1 回だけ使えます)
            </p>
          </div>
        </>
      ) : (
        <p className="agent-panel-status">{ended}</p>
      )}
      <div className="remote-row">
        {ended === null ? null : (
          <button type="button" className="text-button" disabled={busy} onClick={onNew}>
            <Icon name="refresh" size={13} />
            新しいコード
          </button>
        )}
        <IconButton icon="x" size={14} label="コードを隠す" onClick={onHide} />
      </div>
    </section>
  );
}

function DeviceList({
  devices,
  busy,
  confirming,
  onConfirm,
  onRevoke,
}: {
  devices: RemoteDevice[] | null;
  busy: boolean;
  confirming: string | null;
  onConfirm: (device: string | null) => void;
  onRevoke: (device: string) => void;
}) {
  if (devices === null) return null;
  return (
    <section className="remote-devices" aria-label="ペアリングした端末">
      <h4 className="remote-heading">ペアリングした端末</h4>
      {devices.length === 0 ? (
        <p className="agent-panel-status">ペアリングした端末はありません。</p>
      ) : (
        <ul className="secret-list" aria-label="端末">
          {devices.map((d) => (
            <li key={d.id} className="secret-row remote-device" aria-label={d.name}>
              <span className="secret-name">{d.name}</span>
              <span className={`chip ${d.connected ? "tone-done" : ""}`}>{d.connected ? "接続中" : "未接続"}</span>
              <span className="harness-note">最終接続 {d.last_seen_ms === null ? "なし" : formatDateTime(d.last_seen_ms)}</span>
              <span className="harness-note">追加 {formatDateTime(d.created_ms)}</span>
              <span className="spacer" />
              {confirming === d.id ? (
                <>
                  <span className="agent-panel-status">{d.name} を失効しますか? 接続はすぐに切れます。</span>
                  <IconButton icon="trash" size={14} tone="danger" label="失効する" disabled={busy} onClick={() => onRevoke(d.id)} />
                  <IconButton icon="x" size={14} label="やめる" onClick={() => onConfirm(null)} />
                </>
              ) : (
                <IconButton icon="trash" size={14} tone="danger" label={`${d.name} を失効`} onClick={() => onConfirm(d.id)} />
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
