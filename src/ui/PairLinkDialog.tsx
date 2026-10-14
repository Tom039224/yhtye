import { type FormEvent, useEffect, useRef, useState, useSyncExternalStore } from "react";

import type { PairingPreview } from "../api/remoteHosts";
import { toCommandError } from "../api/transport";
import type { HostsRegistry } from "../store/hosts";
import { Icon } from "./Icon";

/**
 * A `yhtye://pair?…` link reached the app (a click on another Yhtye's pairing
 * link or QR code): asks whether to pair with that host before anything is
 * sent — pairing spends the host's one-time code and registers this device
 * there — and under what name this device is listed on the host.
 */
export function PairLinkDialog({ registry }: { registry: HostsRegistry }) {
  const link = useSyncExternalStore(registry.subscribe, () => registry.getState().pairLinks[0] ?? null);
  return link === null ? null : <PairLinkAsk key={link} registry={registry} link={link} />;
}

function PairLinkAsk({ registry, link }: { registry: HostsRegistry; link: string }) {
  const [preview, setPreview] = useState<PairingPreview | null>(null);
  const [name, setName] = useState("");
  // Why the link cannot be used, or why pairing failed.
  const [problem, setProblem] = useState<string | null>(null);
  const [pairing, setPairing] = useState(false);
  const dialog = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let current = true;
    registry
      .previewPairing({ kind: "uri", uri: link })
      .then((p) => {
        if (!current) return;
        setPreview(p);
        setName(p.default_device_name);
      })
      .catch((e: unknown) => {
        if (current) setProblem(toCommandError(e).message);
      });
    return () => {
      current = false;
    };
  }, [registry, link]);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    dialog.current?.focus();
    return () => opener?.focus?.();
  }, []);

  const decline = () => registry.dismissPairLink();
  const pair = async (e: FormEvent) => {
    e.preventDefault();
    if (!preview || pairing || !name.trim()) return;
    setPairing(true);
    setProblem(null);
    try {
      await registry.addHost({ kind: "uri", uri: link }, name.trim());
      registry.dismissPairLink();
    } catch (err) {
      setProblem(toCommandError(err).message);
      setPairing(false);
    }
  };

  return (
    <div className="modal-backdrop">
      <div
        className="pair-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="別の Yhtye に接続"
        tabIndex={-1}
        ref={dialog}
        onKeyDown={(e) => {
          if (e.key === "Escape" && !pairing) decline();
        }}
      >
        <h2 className="pair-title">
          <Icon name="server" size={16} />
          別の Yhtye に接続しますか
        </h2>
        {preview ? (
          <form className="pair-form" onSubmit={(e) => void pair(e)} autoComplete="off">
            <p className="agent-panel-status">
              リンクの接続先に、この PC をクライアントとして登録します。心当たりのないリンクなら「キャンセル」を選んでください。
            </p>
            <dl className="remote-facts">
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
            <label className="pair-name">
              <span>この PC の名前 (接続先の端末一覧に出ます)</span>
              <input
                className="agent-search"
                aria-label="この PC の名前"
                value={name}
                maxLength={64}
                spellCheck={false}
                disabled={pairing}
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            {problem ? (
              <p className="agent-warning" role="alert">
                {problem}
              </p>
            ) : null}
            <div className="remote-row pair-actions">
              <button type="submit" className="text-button" disabled={pairing || !name.trim()}>
                <Icon name={pairing ? "loader" : "check"} size={13} spin={pairing} />
                接続する
              </button>
              <button type="button" className="text-button" disabled={pairing} onClick={decline}>
                キャンセル
              </button>
            </div>
          </form>
        ) : problem ? (
          <>
            <p className="agent-warning" role="alert">
              このリンクは使えません: {problem}
            </p>
            <div className="remote-row pair-actions">
              <button type="button" className="text-button" onClick={decline}>
                閉じる
              </button>
            </div>
          </>
        ) : (
          <p className="agent-panel-status">リンクを確認中…</p>
        )}
      </div>
    </div>
  );
}
