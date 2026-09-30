import { useAppState, useStore } from "../store/useStore";
import { IconButton } from "./Icon";

/** A lost connection and user-visible errors, under the title bar. Never silent. */
export function ConnectionBanner() {
  const connection = useAppState((s) => s.connection);
  const errors = useAppState((s) => s.errors);
  const store = useStore();
  return (
    <div className="banners">
      {connection.state === "closed" ? (
        <div className="alert" role="status" data-testid="connection-lost">
          <span>
            コアとの接続が切れました: {connection.reason}
            {connection.retryInMs !== null ? ` · ${Math.round(connection.retryInMs / 100) / 10} 秒後に再接続` : ""}
          </span>
        </div>
      ) : null}
      {errors.map((e) => (
        <div key={e.id} className="alert" role="alert">
          <span>{e.message}</span>
          <IconButton icon="x" size={14} label="閉じる" onClick={() => store.dismissError(e.id)} />
        </div>
      ))}
    </div>
  );
}
