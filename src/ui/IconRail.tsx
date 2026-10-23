import { Icon, type IconName } from "./Icon";
import { RailProjects } from "./RailProjects";

export type View = "dashboard" | "work";

/**
 * The left rail (design §3.2). It is 44px wide in the layout; hovering it, or
 * moving the keyboard focus into it, expands its panel over the sidebar (CSS
 * only: the labels are always in the DOM). From the top: the dashboard, the
 * projects that are open (see {@link RailProjects}), the work view, and the
 * settings (Stage 7d) at the bottom.
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
  const viewButton = (to: View, icon: IconName, label: string) => (
    <RailButton icon={icon} label={label} active={view === to} pressed onClick={() => onChange(to)} />
  );
  return (
    <div className="rail">
      <nav className="rail-panel" aria-label="views">
        {viewButton("dashboard", "dashboard", "ダッシュボード")}
        <RailProjects working={view === "work"} onShown={() => onChange("work")} />
        <div className="rail-bottom">
          {viewButton("work", "code", "コーディング")}
          <RailButton
            icon="sliders"
            label="設定"
            active={settingsOpen}
            haspopup
            expanded={settingsOpen}
            onClick={onOpenSettings}
          />
        </div>
      </nav>
    </div>
  );
}

interface RailButtonProps {
  icon: IconName;
  label: string;
  active: boolean;
  /** A view switch: `aria-pressed` says whether it is the view shown. */
  pressed?: boolean;
  /** Opens a dialog: `aria-expanded` says whether it is open. */
  haspopup?: boolean;
  expanded?: boolean;
  onClick: () => void;
}

/** An icon with its label (shown when the rail is expanded). */
function RailButton({ icon, label, active, pressed, haspopup, expanded, onClick }: RailButtonProps) {
  return (
    <button
      type="button"
      className={`rail-row rail-button${active ? " active" : ""}`}
      title={label}
      aria-label={label}
      aria-pressed={pressed ? active : undefined}
      aria-haspopup={haspopup ? "dialog" : undefined}
      aria-expanded={haspopup ? expanded : undefined}
      onClick={onClick}
    >
      <span className="rail-icon">
        <Icon name={icon} size={18} />
      </span>
      <span className="rail-label">{label}</span>
    </button>
  );
}
