export type View = "work" | "runs";

/** Switches between the work view and the runs (history) view (design §3.2). */
export function IconRail({ view, onChange }: { view: View; onChange: (v: View) => void }) {
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
    </nav>
  );
}
