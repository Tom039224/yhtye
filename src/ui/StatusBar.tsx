import { useAppState } from "../store/useStore";

/**
 * Connection to the core on the left; the design's usage meters on the right.
 * Usage / quota is not read from the harness yet (Stage 6b): the meters are
 * shown empty with "—", never with made-up values.
 */
export function StatusBar() {
  const connection = useAppState((s) => s.connection);
  const transport = useAppState((s) => s.transport);
  const text =
    connection.state === "open"
      ? `接続済み · ${transport.target}`
      : connection.state === "connecting"
        ? `接続中… · ${transport.target}`
        : `切断: ${connection.reason}${connection.retryInMs !== null ? ` · ${Math.round(connection.retryInMs / 100) / 10} 秒後に再接続` : ""}`;
  const dot = connection.state === "open" ? "dot-ok" : connection.state === "connecting" ? "dot-busy" : "dot-bad";
  return (
    <footer className="statusbar">
      <span className="connection" role="status" data-testid="connection">
        <span className={`dot ${dot}`} />
        {text}
      </span>
      <span className="spacer" />
      <UsageMeter label="5h" />
      <span className="meter-sep" />
      <UsageMeter label="week" />
    </footer>
  );
}

function UsageMeter({ label }: { label: string }) {
  return (
    <span className="meter" title="使用量はまだ取得していません (Stage 6b)" data-testid={`usage-${label}`}>
      <span className="meter-label">{label}</span>
      <span className="meter-bar" />
      <span className="meter-value">—</span>
    </span>
  );
}
