import { useAppState, useStore } from "../store/useStore";

/** Connection state and user-visible errors. Always rendered, never silent. */
export function ConnectionBanner() {
  const connection = useAppState((s) => s.connection);
  const transport = useAppState((s) => s.transport);
  const errors = useAppState((s) => s.errors);
  const store = useStore();

  const text =
    connection.state === "open"
      ? `接続済み · ${transport.target}`
      : connection.state === "connecting"
        ? `接続中… · ${transport.target}`
        : `切断: ${connection.reason}${connection.retryInMs !== null ? ` · ${Math.round(connection.retryInMs / 100) / 10} 秒後に再接続` : ""}`;

  return (
    <div className="banners">
      <div className={`connection connection-${connection.state}`} role="status" data-testid="connection">
        <span className="dot" /> {text}
      </div>
      {errors.map((e) => (
        <div key={e.id} className="alert alert-error" role="alert">
          <span>{e.message}</span>
          <button type="button" className="btn btn-small" onClick={() => store.dismissError(e.id)}>
            閉じる
          </button>
        </div>
      ))}
    </div>
  );
}
