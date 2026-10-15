import { useId, useRef, useState } from "react";

import type { ProjectInfo } from "../api/generated";
import { fillSlots } from "../store/order";
import { useAppState, useStore } from "../store/useStore";
import { Icon } from "./Icon";
import { ProjectMenu } from "./ProjectMenu";
import { backgroundProjects, projectRing, RING_LABEL } from "./projectActivity";
import { reorderKey, useSortable } from "./useSortable";

/**
 * Projects other than the one on screen whose orchestration keeps running in
 * the core, as compact chips under the picker; a click switches to one and a
 * right click opens its menu (sleep, move up / down). Chips can also be dragged
 * onto each other, or moved with Alt + ↑ / ↓. Each chip carries the same ring
 * as the picker (a turning arc while a chat of the project is working, a hollow
 * one otherwise).
 */
export function BackgroundProjects() {
  const store = useStore();
  const projects = useAppState((s) => s.projects);
  const view = useAppState((s) => s.project);
  const connected = useAppState((s) => s.connection.state === "open");
  const currentId = view?.info.id ?? null;
  const background = backgroundProjects(projects, currentId);
  const shown = background.map((p) => p.id);
  // Only these are on screen: they trade the places they have among all the projects.
  const move = (next: string[]) => void store.reorderProjects(fillSlots(projects.map((p) => p.id), next));
  const sortable = useSortable(shown, move, connected);
  if (background.length === 0) return null;
  return (
    <div className="bg-projects">
      <span className="bg-caption" title="バックグラウンドで動作中">
        <Icon name="activity" size={13} />
      </span>
      <ul className="bg-list" aria-label="バックグラウンドで動作中のプロジェクト">
        {background.map((p) => (
          <li key={p.id} {...sortable.rowProps(p.id)}>
            <BackgroundChip project={p} shown={shown} onMove={move} />
          </li>
        ))}
      </ul>
    </div>
  );
}

interface BackgroundChipProps {
  project: ProjectInfo;
  /** The chips on screen, in order. */
  shown: string[];
  onMove: (next: string[]) => void;
}

function BackgroundChip({ project: p, shown, onMove }: BackgroundChipProps) {
  const store = useStore();
  const view = useAppState((s) => s.project);
  const opening = useAppState((s) => s.busy.opening);
  const connected = useAppState((s) => s.connection.state === "open");
  const [menuOpen, setMenuOpen] = useState(false);
  const chipRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const ring = projectRing(p, view);
  return (
    <>
      <button
        ref={chipRef}
        type="button"
        className="bg-chip"
        title={`${p.path} · ${RING_LABEL[ring]}`}
        aria-label={`${p.name} に切り替え (${RING_LABEL[ring]})`}
        disabled={!connected || opening}
        onClick={() => void store.openProject(p.path)}
        onKeyDown={(e) => void reorderKey(e, shown, p.id, onMove)}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenuOpen(true);
        }}
      >
        <span className={`ring ring-${ring}`} data-testid={`bg-ring-${p.id}`} aria-hidden="true" />
        <span className="name">{p.name}</span>
      </button>
      {menuOpen ? (
        <ProjectMenu
          id={menuId}
          project={p}
          anchor={chipRef}
          order={{ keys: shown, onMove, onMoved: () => chipRef.current?.focus() }}
          onClose={(refocus) => {
            setMenuOpen(false);
            if (refocus) chipRef.current?.focus();
          }}
        />
      ) : null}
    </>
  );
}
