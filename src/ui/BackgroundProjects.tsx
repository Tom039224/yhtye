import { useAppState, useStore } from "../store/useStore";
import { Icon } from "./Icon";
import { backgroundProjects, projectRing, RING_LABEL } from "./projectActivity";

/**
 * Projects other than the one on screen whose orchestration keeps running in
 * the core, as compact chips under the picker; a click switches to one. Each
 * chip carries the same ring as the picker (a turning arc while a chat of the
 * project is working, a hollow one otherwise).
 */
export function BackgroundProjects() {
  const store = useStore();
  const projects = useAppState((s) => s.projects);
  const view = useAppState((s) => s.project);
  const currentId = view?.info.id ?? null;
  const opening = useAppState((s) => s.busy.opening);
  const connected = useAppState((s) => s.connection.state === "open");
  const background = backgroundProjects(projects, currentId);
  if (background.length === 0) return null;
  return (
    <div className="bg-projects">
      <span className="bg-caption" title="バックグラウンドで動作中">
        <Icon name="activity" size={13} />
      </span>
      <ul className="bg-list" aria-label="バックグラウンドで動作中のプロジェクト">
        {background.map((p) => {
          const ring = projectRing(p, view);
          return (
            <li key={p.id}>
              <button
                type="button"
                className="bg-chip"
                title={`${p.path} · ${RING_LABEL[ring]}`}
                aria-label={`${p.name} に切り替え (${RING_LABEL[ring]})`}
                disabled={!connected || opening}
                onClick={() => void store.openProject(p.path)}
              >
                <span className={`ring ring-${ring}`} data-testid={`bg-ring-${p.id}`} aria-hidden="true" />
                <span className="name">{p.name}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
