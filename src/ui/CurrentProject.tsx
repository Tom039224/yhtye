import { useActiveHostEntry } from "../store/useHosts";
import { useAppState } from "../store/useStore";
import { projectRing, RING_LABEL } from "./projectActivity";
import { RemoteMark } from "./RemoteMark";

/**
 * The project on screen, by name (the sidebar's "Project" section). Projects
 * are chosen in the rail and on the dashboard, so this only shows which one the
 * sidebar below belongs to: its ring (turning while work is on), its name and,
 * for another host's project, the remote mark.
 */
export function CurrentProject() {
  const view = useAppState((s) => s.project);
  const entry = useActiveHostEntry();
  if (!view) {
    return (
      <div className="project-current">
        <span className="name placeholder">プロジェクトが開かれていません</span>
      </div>
    );
  }
  const ring = projectRing(view.info, view);
  const host = entry?.remote ? `${entry.name ?? entry.key}: ` : "";
  return (
    <div className="project-current" title={`${host}${view.info.path} · ${RING_LABEL[ring]}`}>
      <span className={`ring ring-${ring}`} data-testid="current-ring" />
      <span className="name">{view.info.name}</span>
      <RemoteMark entry={entry ?? undefined} />
    </div>
  );
}
