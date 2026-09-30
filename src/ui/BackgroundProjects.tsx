import { useAppState, useStore } from "../store/useStore";
import { backgroundProjects } from "./projectActivity";

/**
 * Projects other than the one on screen whose orchestration keeps running in
 * the core, as compact chips under the picker; a click switches to one.
 */
export function BackgroundProjects() {
  const store = useStore();
  const projects = useAppState((s) => s.projects);
  const currentId = useAppState((s) => s.project?.info.id ?? null);
  const opening = useAppState((s) => s.busy.opening);
  const connected = useAppState((s) => s.connection.state === "open");
  const background = backgroundProjects(projects, currentId);
  if (background.length === 0) return null;
  return (
    <div className="bg-projects">
      <div className="bg-caption">動作中</div>
      <ul className="bg-list" aria-label="バックグラウンドで動作中のプロジェクト">
        {background.map((p) => (
          <li key={p.id}>
            <button
              type="button"
              className="bg-chip"
              title={`${p.path} · バックグラウンドで動作中`}
              aria-label={`${p.name} に切り替え (動作中)`}
              disabled={!connected || opening}
              onClick={() => void store.openProject(p.path)}
            >
              <span className="bg-dot" data-testid={`bg-ring-${p.id}`} aria-hidden="true" />
              <span className="name">{p.name}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
