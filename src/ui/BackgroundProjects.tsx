import { useRef, useState } from "react";

import { fillSlots } from "../store/order";
import { useAppState, useStore } from "../store/useStore";
import { Icon } from "./Icon";
import { backgroundProjects, projectRing, RING_LABEL } from "./projectActivity";
import { RowMenu } from "./RowMenu";
import { useContextMenu } from "./useContextMenu";
import { moveActions, reorderKey, useSortable } from "./useSortable";

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
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const menu = useContextMenu();
  const chipRefs = useRef(new Map<string, HTMLButtonElement>());
  const background = backgroundProjects(projects, currentId);
  const shown = background.map((p) => p.id);
  // Only these are on screen: they trade the places they have among all the projects.
  const move = (next: string[]) =>
    void store.reorderProjects(fillSlots(projects.map((p) => p.id), next));
  const sortable = useSortable(shown, move, connected);
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
            <li key={p.id} {...sortable.rowProps(p.id)} onContextMenu={(e) => {
              setMenuFor(p.id);
              menu.onContextMenu(e);
            }}>
              <button
                ref={(el) => {
                  if (el) chipRefs.current.set(p.id, el);
                  else chipRefs.current.delete(p.id);
                }}
                type="button"
                className="bg-chip"
                title={`${p.path} · ${RING_LABEL[ring]}`}
                aria-label={`${p.name} に切り替え (${RING_LABEL[ring]})`}
                disabled={!connected || opening}
                onClick={() => void store.openProject(p.path)}
                onKeyDown={(e) => void reorderKey(e, shown, p.id, move)}
              >
                <span className={`ring ring-${ring}`} data-testid={`bg-ring-${p.id}`} aria-hidden="true" />
                <span className="name">{p.name}</span>
              </button>
            </li>
          );
        })}
      </ul>
      {menu.isOpen && menuFor ? (
        <RowMenu
          id="bg-project-menu"
          label={`${background.find((p) => p.id === menuFor)?.name ?? ""} の操作`}
          anchor={menu.anchor}
          actions={moveActions(shown, menuFor, move)}
          onClose={(refocus) => {
            menu.close();
            if (refocus) chipRefs.current.get(menuFor)?.focus();
          }}
        />
      ) : null}
    </div>
  );
}
