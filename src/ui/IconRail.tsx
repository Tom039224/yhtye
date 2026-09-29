export type View = "work" | "runs";

/**
 * Switches between the work view and the runs (history) view (design §3.2); the
 * settings button (Stage 7d) sits at the bottom of the strip.
 */
export function IconRail({
  view,
  onChange,
  settingsOpen,
  onOpenSettings,
}: {
  view: View;
  onChange: (v: View) => void;
  settingsOpen: boolean;
  onOpenSettings: () => void;
}) {
  return (
    <nav className="rail" aria-label="views">
      <button
        type="button"
        className={`rail-button ${view === "work" ? "active" : ""}`}
        title="コーディング"
        aria-label="コーディング"
        aria-pressed={view === "work"}
        onClick={() => onChange("work")}
      >
        &lt;/&gt;
      </button>
      <button
        type="button"
        className={`rail-button runs ${view === "runs" ? "active" : ""}`}
        title="履歴"
        aria-label="履歴"
        aria-pressed={view === "runs"}
        onClick={() => onChange("runs")}
      >
        ▤
      </button>
      <span className="spacer" />
      <button
        type="button"
        className={`rail-button settings ${settingsOpen ? "active" : ""}`}
        title="設定"
        aria-label="設定"
        aria-haspopup="dialog"
        aria-expanded={settingsOpen}
        onClick={onOpenSettings}
      >
        ⚙
      </button>
    </nav>
  );
}
