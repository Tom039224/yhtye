import { useActiveHostEntry } from "../store/useHosts";
import { useAppState, useStore } from "../store/useStore";
import { compatibilityNote } from "./hostLabels";
import { Icon, IconButton } from "./Icon";

/**
 * A lost connection, a remote host whose version does not fit, and
 * user-visible errors, under the title bar. Never silent. For a remote host on
 * screen the messages name it.
 */
export function ConnectionBanner() {
  const connection = useAppState((s) => s.connection);
  const compatibility = useAppState((s) => s.host.compatibility);
  const errors = useAppState((s) => s.errors);
  const store = useStore();
  const host = useActiveHostEntry();
  const remoteName = host?.remote ? (host.name ?? host.key) : null;
  const versionNote = remoteName ? compatibilityNote(compatibility) : null;
  return (
    <div className="banners">
      {connection.state === "closed" ? (
        <div className="alert" role="status" data-testid="connection-lost">
          <Icon name="plug-off" size={14} />
          <span className="alert-text">
            {remoteName ? `${remoteName} との接続が切れました` : "コアとの接続が切れました"}: {connection.reason}
            {connection.retryInMs !== null ? ` · ${Math.round(connection.retryInMs / 100) / 10} 秒後に再接続` : ""}
          </span>
        </div>
      ) : null}
      {versionNote ? (
        <div className="alert" role="status" data-testid="host-incompatible">
          <Icon name="alert" size={14} />
          <span className="alert-text">
            {remoteName} の Yhtye: {versionNote}
          </span>
        </div>
      ) : null}
      {errors.map((e) => (
        <div key={e.id} className="alert" role="alert">
          <Icon name="alert" size={14} />
          <span className="alert-text">{e.message}</span>
          <IconButton icon="x" size={14} label="閉じる" onClick={() => store.dismissError(e.id)} />
        </div>
      ))}
    </div>
  );
}
